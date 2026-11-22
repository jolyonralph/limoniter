//! The window: ribbon, tabs, status bar, panels, dialogs and file handling.

use crate::associations;
use crate::char_table;
use crate::charset::{self, Charset};
use crate::columns;
use crate::status_items;
use crate::conversions;
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
use crate::side_tabs;
use crate::tabs;
use crate::text_buffer::{self, CaseChange, Justify, ReplaceCursor, SearchCursor, SearchOptions, SearchStep, TextBuffer};
use crate::theme;
use crate::scripting;
use crate::text_objects;
use crate::tools;
use crate::views::{self, ClipState, ContextCommand, Pane, ViewOptions, ViewState};
use eframe::egui::{self, Button, Context, Key, KeyboardShortcut, Modifiers, Panel, RichText, Ui, ViewportCommand};
use rfd::{MessageButtons, MessageDialogResult, MessageLevel};
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
/// How often open files are looked at for changes, when that's asked for as well as on coming back into focus.
const WATCH_EVERY: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    /// Folds the ribbon to its tabs, or opens it out again.
    FoldRibbon,
    /// The box that chooses the toolbar's buttons.
    ToolbarButtons,
    /// The newest notes from What's new.
    WhatsNew,
    /// One of the guide's pages.
    Guide(start_page::Guide),
    /// The Git wizard, set to copy a project from online.
    GitClone,
    Checksums,
    RomPatches,
    Entropy,
    RegexTester,
    SchemaCheck,
    FlashSettings,
    SerialTerminal,
    EpromProgrammer,
    CodePicture,
    DocComment,
    MarkdownToc,
    MarkdownTable,
    Formatters,
    ServeFolder,
    /// Tools, PHP.
    Php(php_host::PhpAction),
    /// Tools, Python and R: the console, a script's cells, environments and knitting.
    PyR(python_host::PyAction),
    TimeSpent,
    HowToCode,
    HowToCodeExamples,
    HowToCodeSpectrum,
    HowToCodeSpectrumExamples,
    New,
    Open,
    Save,
    SaveAs,
    CloseTab,
    /// File, Close all: every open file, asking about each unsaved one.
    CloseAll,
    /// File, Close the others: every file but the one in front.
    CloseOthers,
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
    /// Shows another tab beside the one in front, or stops.
    OpenBeside,
    /// The Search panel: find (and replace) in the folder's files, the results as a tree.
    SearchPanel,
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
    Convert(conversions::Conversion),
    SaveSettingsFile,
    SaveBbcZ80Program,
    SaveAmosProgram,
    SaveStosProgram,
    SaveLayout,
    SaveDocumentAs(crate::legacy_docs::Target),
    GatherWindows,
    Surround(char, char),
    SurroundTag,
    /// Edit, Surround with, An Emmet abbreviation.
    WrapAbbreviation,
    MoveToBin,
    LeadingTabsToSpaces,
    ClearBookmarks,
    InvertBookmarks,
    CopyBookmarked,
    CutBookmarked,
    DeleteBookmarked,
    DeleteUnbookmarked,
    CheckUpdates,
    GoBackVersion,
    RestoreSettingsFile,
    ProjectOpen,
    ProjectNew,
    /// Opens a folder as a project, as VS Code does.
    OpenFolder,
    /// Finds a file by a few letters of its name.
    GoToFile,
    /// Finds a function or label: in the file in front, or (true) in the folder.
    GoToSymbol(bool),
    /// Has the language server lay out the whole file.
    FormatDocument,
    /// Offers the language server's fixes for the cursor's line.
    QuickFix,
    /// Shows where the word at the cursor is defined, in a window, without going there.
    PeekDefinition,
    /// Debugs the file in front with its language's debugger (or carries on, while one's paused).
    StartDebugging,
    /// Waits for Xdebug to connect from a PHP page in the browser, or a script run from the terminal.
    DebugPhpListen,
    /// Debugs with one of the setups in the folder's `.vscode/launch.json`, picked from a list.
    DebugVscodeSetup,
    /// Steps back to the line before in the program being debugged, where the debugger can go backwards
    /// (Copperline's can, by replaying its snapshots).
    DebugStepBack,
    /// The Colour as box: the language the file in front is coloured as.
    ColourAs,
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
    /// The window's own font and size, apart from the text's.
    WindowFont,
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
    /// VS Code extensions: installed, turned on and off, and removed.
    Extensions,
    NewAddOn,
    OpenRemote,
    SaveRemote,
    /// A folder on a server, opened as the project.
    OpenRemoteFolder,
    /// File, Open a ZIP as a project; Open a project from a web address.
    OpenZipProject,
    OpenWebProject,
    /// The project's server looked at for changes now, or a sync shown to be agreed to.
    RemoteProjectFetch,
    RemoteProjectSync,
    SendTo(usize),
    RunOnMachine,
    RunOnBuiltIn,
    RunOnBuiltIn2600,
    RunOnBuiltInVectrex,
    RunOn(usize),
    StopRunning,
    ChooseAssembler,
    /// Which compilers (Boriel's ZX Basic, C for the older machines) are installed, and where to get them.
    Compilers,
    AssembleAndSave,
    AssembleBinary,
    /// A new file of a kind from the New as list, by its place in it.
    NewAs(usize),
    /// File, Quit: the window closed as its own close button closes it, asking about unsaved work first.
    Quit,
    /// A new file from one of the usual New as kinds, by its name (Tools, Early computing).
    NewUsual(&'static str),
    EditNewKinds,
    /// File, Project: a new file or files from one of the project's own templates, the one at this place in its list.
    NewFromTemplate(usize),
    /// File, Project: the file in front kept as one of the project's templates.
    SaveAsTemplate,
    /// File, Project: how this project saves files by itself (None leaves it to the settings).
    ProjectAutosave(Option<settings::AutosaveChoice>),
    /// For the label at the cursor in assembler source.
    GoToDefinition,
    FindUses,
    RenameLabel,
    Disassemble,
    /// Saves what an Amiga file crunched with PowerPacker, Imploder, StoneCracker, CrunchMania or ByteKiller
    /// decrunches to: the data, or the program built again.
    SaveDecrunched,
    NewImage,
    OpenUrl,
    AmigaNdk,
    AutoDocs,
    AmigaTools,
    AmigaIcon,
    /// A new Amiga C project from a template, in a folder of its own.
    AmigaNewProject,
    /// A folder of Amiga C (SAS/C's among it) opened as a project, sorted and given makefiles.
    AmigaImportProject,
    /// The Amiga project's makefiles written again from its files.
    AmigaMakefiles,
    /// The Amiga catalog translation in front checked against its description, or the description's translations.
    CatalogCheck,
    /// Each translation's catalog and the strings' C header, made by Limoniter.
    CatalogMake,
    /// Each translation's catalog made by FlexCat.
    CatalogFlexCat,
    /// A translation for the description in front, new or with the strings it's missing added.
    CatalogTranslation,
    HatariProfile,
    StFloppy,
    StTools,
    C64Tools,
    Clambers,
    CheckOpticos,
    CleanRecent,
    ClearRecent,
    PixelEditor,
    /// The tile map and level editor's window.
    TileMapEditor,
    ScreenDesigner,
    AmigaHardware,
    AmigaBitplanes,
    StPictures,
    C64Pictures,
    RetroPictures,
    NextPainter,
    Atari2600Graphics,
    CartView,
    SoundEffects,
    Ripper,
    AmigaBootDisk,
    /// The Dragon DOS disk window: files put on a disk for the Dragon 32 and 64.
    DragonDisk,
    BlitterMinterms,
    TableGenerator,
    AtariHardware,
    AtariBlitter,
    AtariTimers,
    C64RasterLines,
    C64RasterIrqs,
    C64Hardware,
    ToggleTerminal,
    /// A task, by its place in the list of those here.
    RunTask(usize),
    /// A task that runs a `package.json` script, run again under js-debug, by its place in the list of those here.
    DebugTask(usize),
    BuildTask,
    TestTask,
    /// The assembly tests written beside the source in front, run on the built-in Z80 or 6502.
    AssemblyTests,
    /// The raster and copper designer: colour bars and splits made into a copper list, raster interrupts or
    /// Timer B code.
    RasterDesigner,
    EditTasks,
    /// Asks each package manager the project uses what's out of date.
    OutdatedPackages,
    /// Sends the request the cursor is in, in a `.http` file.
    SendRequest,
    /// Sends the numbered request of the `.http` file, from its link in the editor.
    SendRequestNumber(usize),
    /// Forgets the cookies a `.http` file's responses have set.
    HttpClearCookies,
    MachineMemory,
    MachineScreen,
    MachineBasicOut,
    MachineBasicIn,
    SourceControl,
    GitWizard,
    GitCompareHead,
    /// Opens a tab comparing two folders, revisions, images or folders on a server.
    CompareFolders,
    /// Asks for two files and compares them side by side.
    CompareFiles,
    /// Asks for a patch, then checks it against a folder or file before applying it.
    ApplyPatch,
    /// Lists the saved folder comparisons, to open, rename or delete.
    CompareSessions,
    GitHistory,
    GitStageChange,
    GitUndoChange,
    /// Merges three files, chosen one after another, into a merge tab.
    Merge,
    /// Opens the file in front, which Git stopped a merge in or which has conflict markers, in a merge tab.
    ResolveInMerge,
    /// Sets Git to use Limoniter for git difftool and git mergetool, or puts back what it used before.
    GitUseTools,
    GitStopTools,
    Themes,
    Website,
    ToggleTable,
    /// View, Open in the data viewer: the delimited text in front, as a table to sort, filter and sum up.
    OpenInDataViewer,
    QueryRows,
    /// Something to do with the notebook in front: run a cell, add one, and so on.
    Notebook(notebook_host::Act),
    /// Something to do with databases: run the SQL at the cursor, stop it, add a connection.
    Database(database_host::Act),
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
    TapeSound,
    /// Saves the preview (Markdown, a web page, a converted document) as a PDF.
    SavePreviewAsPdf,
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
    /// Which clipboard Copy, Cut and Paste use: 0 the computer's, 1 to 9 Limoniter's own.
    UseClipboard(u8),
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
        // The PHP tools that work on a whole project.
        matches!(self, Command::Php(action) if action.works_without_file())
            || matches!(self, Command::Database(act) if act.works_without_file())
            || matches!(self, Command::PyR(action) if action.works_without_file())
            || matches!(
            self,
            Command::New
                | Command::FoldRibbon
                | Command::ToolbarButtons
                | Command::WhatsNew
                | Command::Guide(_)
                | Command::GitClone
                | Command::Clambers
                | Command::AmigaNdk
                | Command::AmigaNewProject
                | Command::AmigaImportProject
                | Command::AmigaMakefiles
                | Command::AutoDocs
                | Command::AmigaHardware
                | Command::AmigaTools
                | Command::StTools
                | Command::C64Tools
                | Command::CheckOpticos
                | Command::Open
                | Command::OpenFolder
                | Command::GoToFile
                | Command::GoToSymbol(true)
                | Command::SearchPanel
                | Command::DragonDisk
                | Command::Checksums
                | Command::RomPatches
                | Command::Entropy
                | Command::RegexTester
                | Command::SchemaCheck
                | Command::FlashSettings
                | Command::SerialTerminal
                | Command::EpromProgrammer
                | Command::Formatters
                | Command::ServeFolder
                | Command::TimeSpent
                | Command::HowToCode
                | Command::HowToCodeExamples
                | Command::HowToCodeSpectrum
                | Command::HowToCodeSpectrumExamples
                | Command::AmigaBootDisk
                | Command::NewImage
                | Command::OpenUrl
                | Command::UseClipboard(_)
                | Command::Features
                | Command::Disassemble
                | Command::SaveDecrunched
                | Command::BuildTask
                | Command::TestTask
                | Command::RasterDesigner
                | Command::EditTasks
                | Command::OutdatedPackages
                | Command::DebugPhpListen
                | Command::DebugVscodeSetup
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
                | Command::WindowFont
                | Command::ChoosePreset
                | Command::ShowFileLists
                | Command::StopRunning
                | Command::ChooseAssembler
                | Command::Compilers
                | Command::AssembleAndSave
                | Command::AssembleBinary
                | Command::NewAs(_)
                | Command::NewFromTemplate(_)
                | Command::PixelEditor
                | Command::TileMapEditor
                | Command::ScreenDesigner
                | Command::AmigaBitplanes
                | Command::StPictures
                | Command::C64Pictures
                | Command::RetroPictures
                | Command::NextPainter
                | Command::Atari2600Graphics
                | Command::CartView
                | Command::SoundEffects
                | Command::Ripper
                | Command::BlitterMinterms
                | Command::TableGenerator
                | Command::AtariHardware
                | Command::AtariBlitter
                | Command::AtariTimers
                | Command::C64RasterLines
                | Command::C64RasterIrqs
                | Command::C64Hardware
                | Command::ToggleTerminal
                | Command::EditNewKinds
                | Command::MachineMemory
                | Command::MachineScreen
                | Command::MachineBasicOut
                | Command::SourceControl
                | Command::GitWizard
                | Command::Merge
                | Command::GitUseTools
                | Command::GitStopTools
                | Command::Settings
                | Command::SaveSettingsFile
                | Command::SaveLayout
                | Command::GatherWindows
                | Command::CheckUpdates
                | Command::GoBackVersion
                | Command::RestoreSettingsFile
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
                | Command::Extensions
                | Command::NewAddOn
                | Command::OpenRemote
                | Command::OpenRemoteFolder
                | Command::OpenZipProject
                | Command::OpenWebProject
                | Command::RemoteProjectFetch
                | Command::RemoteProjectSync
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
                | Command::CompareFolders
                | Command::CompareFiles
                | Command::ApplyPatch
                | Command::CompareSessions
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
                | Command::Convert(_)
                | Command::Surround(..)
                | Command::SurroundTag
                | Command::WrapAbbreviation
                | Command::CutBookmarked
                | Command::DeleteBookmarked
                | Command::DeleteUnbookmarked
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
                | Command::LeadingTabsToSpaces
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

/// Another tab shown beside the one in front, as VS Code's editor groups are. Each stays on its side: clicking
/// in the other one brings it to the front, there.
struct Beside {
    /// The tab shown beside, and the one last in front.
    other: u64,
    front: u64,
    /// Whether the other one is on the right.
    right: bool,
    /// How much of the width the left one has.
    share: f32,
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
    /// For a page of a How To Code guide, the pages it was reached from (the last the one before) and those gone
    /// back from, as a browser keeps them: a link in a guide opens the page in the same tab.
    guide_back: Vec<PathBuf>,
    guide_forward: Vec<PathBuf>,
    text: TextBuffer,
    hex: HexBuffer,
    path: Option<PathBuf>,
    untitled: String,
    hex_mode: bool,
    /// Shown as a table of delimited data, and how that's read.
    table: Option<Box<table_view::TableState>>,
    /// A table of data shown in the data viewer, which the tab then is.
    data: Option<Box<crate::data_view::DataState>>,
    /// A Jupyter notebook's cells, kept for an `.ipynb` file shown as a notebook.
    notebook: Option<Box<notebook_host::NotebookState>>,
    /// The notebook's JSON shown as text instead (View, Notebook as text).
    notebook_text: bool,
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
    /// The old word processor whose document this is, when it's opened by itself.
    legacy: Option<crate::legacy_docs::Kind>,
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
    /// For 6502 source whose code doesn't say: the machine chosen when F9 asked.
    machine_6502: Option<crate::asm::output::Micro6502>,
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
    /// The hidden game, playing in the text's place.
    game: Option<Box<clambers_host::Playing>>,
    /// A merge of three versions, this document being its result.
    merge: Option<Box<merge_host::MergeTab>>,
    /// A comparison of files or folders, showing in the text's place.
    compare: Option<Box<compare_host::CompareTab>>,
}

/// Anything that changes what a recovery copy would hold.
type RecoveryKey = (u64, bool, Eol, usize, bool, Display);

/// One copy found from an earlier run, as offered to the person.
struct OfferItem {
    file: PathBuf,
    selected: bool,
    /// Whether the file has been changed by something else since the copy was made, so bringing the copy back
    /// would be a choice.
    changed_on_disk: bool,
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
            guide_back: Vec::new(),
            guide_forward: Vec::new(),
            picture: None,
            legacy: None,
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
            data: None,
            notebook: None,
            notebook_text: false,
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
            machine_6502: None,
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
            game: None,
            merge: None,
            compare: None,
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
                    doc.hex.notice = Some(crate::i18n::tf!("{} has changed since these edits were made, so only the bytes that were typed or pasted in could be kept.", meta.name));
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
                let store = crate::line_store::LineStore::from_image(image, source).ok_or(crate::i18n::tr("It needs the original file, which can't be read."))?;
                doc.text.adopt_recovered(store, meta.caret);
            }
        }
        Ok(doc)
    }

    /// A very large file's copy only holds what was edited. If the file itself has since changed, the
    /// untouched lines are gone, so what's left is each edited stretch, labelled with where it was.
    fn edited_lines_only(meta: &recovery::Meta, image: crate::line_store::StoreImage, mut doc: Self) -> Self {
        let mut lines = vec![crate::i18n::tf!("The file {} has changed since these edits were made, so only the edited lines could be kept.", meta.name)];
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
                doc.hex.notice = Some(crate::i18n::tf!("{} is too large to read as text in its encoding, so it's shown in hex. Text files in UTF-8 or a byte set can be any size.", doc.name()));
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
        let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        doc.picture = crate::pictures::detect(&file_name, &bytes);
        doc.legacy = if doc.picture.is_none() { crate::legacy_docs::detect_opened(&file_name, &bytes) } else { None };
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
        if let Some(compare) = &self.compare {
            return compare.unsaved();
        }
        if let Some(narrow) = &self.narrow {
            return self.carried || narrow.outer.is_modified() || self.text.version() != narrow.synced;
        }
        self.carried || if self.hex_mode { self.hex.is_modified() } else { self.text.is_modified() }
    }

    /// The wording for the status bar.
    fn status_file_parts(&self) -> (Option<String>, String, bool) {
        let meta = self.path.as_ref().and_then(|p| std::fs::metadata(p).ok());
        let modified = meta.as_ref().map(|m| crate::i18n::tf!("Last saved {}", panels::format_time(m.modified())));
        let bytes = meta.as_ref().map_or(0, std::fs::Metadata::len);
        let read_only = meta.as_ref().is_some_and(|m| m.permissions().readonly());
        let size = if self.hex_mode {
            short_size(self.hex.len() as u64)
        } else {
            // This is about the file, so while narrowed it counts all of the file's lines.
            let lines = self.narrow.as_ref().map_or(self.text.line_count(), |narrow| narrow.outer.line_count() + self.text.line_count() - (narrow.bottom - narrow.top + 1));
            if lines == 1 { crate::i18n::tf!("{} line \u{B7} {}", lines, short_size(bytes)) } else { crate::i18n::tf!("{} lines \u{B7} {}", lines, short_size(bytes)) }
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

    /// The grammar the document is coloured with, working it out if it hasn't been yet.
    fn syntax_index(&mut self) -> Option<usize> {
        self.update_syntax();
        self.resolved_syntax.flatten()
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
        self.placeholder || self.path.is_none() && self.compare.is_none() && self.data.is_none() && !self.is_modified() && !self.hex_mode && self.text.line_count() == 1 && self.text.line(0).is_empty()
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
    /// Matches in comments and strings don't count.
    code_only: bool,
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

/// Whether a file is a page of one of the How To Code guides Limoniter unpacks into its data folder.
fn is_guide_page(path: &Path) -> bool {
    let Some(data) = crate::settings::data_folder() else { return false };
    path.extension().is_some_and(|ending| ending.eq_ignore_ascii_case("md")) && ["howtocode", "howtocode-spectrum"].iter().any(|guide| path.starts_with(data.join(guide)))
}

/// The About box. Returns whether OK was clicked, and whether the list of what Limoniter can do was asked for.
/// Opens the links the frame's hyperlinks asked for (the About box's, a tool's website), as Limoniter opens web
/// pages. eframe is built without a way of its own, so without this a click on a link did nothing.
fn open_links(ctx: &Context) {
    let mut links = Vec::new();
    ctx.output_mut(|output| {
        output.commands.retain(|command| match command {
            egui::OutputCommand::OpenUrl(link) => {
                links.push(link.url.clone());
                false
            }
            _ => true,
        });
    });
    for link in links {
        if let Err(problem) = crate::platform::open_web_page(&link) {
            warn(&problem);
        }
    }
}

/// The About box's page. Gives whether OK, What Limoniter can do and Licence information were pressed.
fn about_page(ui: &mut Ui, notices: Option<&Path>) -> (bool, bool, bool) {
    let (mut ok, mut features) = (false, false);
    ui.set_width(560.0);
    ui.add_space(4.0);
    // An 80s demo screen: the logo, copper bars and a scroller saying what Limoniter is.
    crate::about_demo::show(ui, crate::i18n::tr("A full IDE for today's code and yesterday's computers. Write, build and debug in Python, C, Rust and the rest, with language servers and Git. Assemble for the Amiga, the ZX Spectrum, the Commodore 64, the Atari ST and more, and run it on a built-in machine or an emulator Limoniter installs for you. Edit text and bytes of any size, and explore the disks, tapes and archives of classic computers."));
    ui.add_space(6.0);
    ui.vertical_centered(|ui| ui.label(RichText::new(crate::i18n::tf!("Version {}", env!("CARGO_PKG_VERSION"))).size(12.5).color(theme::text_dim())));
    // A portable copy says where it keeps everything, since that's beside it rather than in the usual place.
    if let Some(folder) = settings::portable_folder() {
        ui.vertical_centered(|ui| ui.label(RichText::new(crate::i18n::tf!("Portable: everything is kept in {}", folder.display())).size(12.0).color(theme::text_dim())));
    }
    ui.add_space(12.0);
    ui.separator();
    ui.add_space(6.0);
    ui.add(
        egui::Label::new(
            RichText::new(crate::i18n::tr(
                "Work on a folder of code as in VS Code, with language servers, debugging, Git and terminals; assemble Z80, 6502, 6809 and 680x0 code and run it on built-in machines or emulators, with How To Code v8 to learn the Amiga from. Edit text and bytes of any size, in the character sets of machines from mainframes to home computers, and open the disk images, tapes and archives of the Amiga, Commodore 64, ZX Spectrum, Acorn, Atari ST, Macintosh, CP/M machines and more: browse their files, list their BASIC programs and see their pictures, then save any of it in today's formats. Nothing is ever written back to an image.",
            ))
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
        (crate::pictures::Format::ALL.iter().filter(|f| !matches!(f, crate::pictures::Format::Png | crate::pictures::Format::Jpeg)).count().to_string(), "legacy picture formats"),
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
        ui.label(RichText::new(crate::i18n::tr("Created by")).size(14.0).color(theme::text_dim()));
        ui.label(RichText::new(crate::i18n::tr("Jolyon Ralph")).size(14.0).strong().color(theme::strong_text()));
    });
    ui.hyperlink_to(RichText::new(crate::i18n::tr("github.com/jolyonralph/limoniter")).size(13.0), "https://github.com/jolyonralph/limoniter");
    ui.add_space(10.0);
    ui.add(egui::Label::new(RichText::new(crate::i18n::tr("Limoniter is free software under the Apache License 2.0.")).size(11.5).color(theme::text_faint())).wrap());
    ui.add_space(10.0);
    let mut licence = false;
    ui.horizontal(|ui| {
        // The licences of the work Limoniter is built on, in a file installed beside it.
        if let Some(path) = notices {
            licence = ui.button(crate::i18n::tr("Licence information\u{2026}")).on_hover_text(crate::i18n::tr("The licences of Limoniter and of the work it's built on")).on_hover_text(path.display().to_string()).clicked();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ok = ui.button(RichText::new(crate::i18n::tr("   OK   "))).clicked();
            features = ui.button(crate::i18n::tr("What Limoniter can do\u{2026}")).on_hover_text(crate::i18n::tr("The list of everything Limoniter can do, as a page of its own")).clicked();
        });
    });
    (ok, features, licence)
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
    Extensions(Box<vsix_host::ExtensionsBox>),
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
    /// A folder project's settings hold what's hidden in its Explorer, to take things back.
    ProjectSettings(Box<project_settings_host::Draft>),
    RestorePick { backups: Vec<project::Backup>, chosen: usize },
    Fill { value: String, skip_last: bool },
    /// Before saving a BASIC listing as a program: which Commodore machine, or which line a Spectrum program runs from.
    SaveBasic { machine: basic::Machine, run_from: String, error: String },
    ColumnRemove { cut: bool, count: String, error: String, focus: bool },
    CsvToFixed { rows: (usize, usize), delimiter: char, error: String },
    FixedToCsv { rows: (usize, usize), delimiter: char, starts: String, error: String, focus: bool },
    Number { first: String, step: String, error: String },
    Message { title: String, body: String },
    /// Column mode asked for while Word wrap is on, which it can't be used with: an offer to turn Word wrap off.
    ColumnNeedsNoWrap,
    /// Window, Layouts, Save this layout: its name, and what's wrong with it.
    SaveLayout { name: String, error: String },
    /// Edit, Surround with, A tag: what's typed, and what's wrong with it.
    SurroundTag { value: String, error: String },
    /// Edit, Surround with, An Emmet abbreviation: what's typed, and what's wrong with it.
    WrapAbbreviation { value: String, error: String },
    /// A newer Limoniter is out: its version, the start of its notes, and how far its update has got.
    Update(Box<update_host::Offer>),
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
    /// The window's font and size.
    WindowFont { error: String },
    Color { rgb: [u8; 3], form: usize },
    Rename { name: String, error: String, focus: bool },
    SplitFile { every: String, by_text: bool, text: String, error: String },
    Snippets(file_tools::SnippetBox),
    RunTool { line: String, save_first: bool, error: String, focus: bool },
    Assembler(Box<asm_host::AssemblerBox>),
    /// The compilers installed, and where to get the rest.
    Compilers(Vec<(&'static crate::compilers::Compiler, Option<PathBuf>)>),
    /// F9 on Boriel's ZX Basic with no zxbc: why, and an offer to install it.
    ZxbcMissing(String),
    /// Debugging JavaScript with js-debug not installed: an offer to download it.
    JsDebugMissing,
    NewKinds(Box<new_kinds::NewKindsBox>),
    Template(Box<templates_host::TemplateBox>),
    SaveTemplate(Box<templates_host::SaveTemplateBox>),
    /// Saving a folder comparison by name, or looking after those saved.
    CompareSession(Box<compare_host::sync_host::SessionBox>),
    CompareRules(Box<compare_host::sync_host::RulesBox>),
    ApplyPatch(Box<compare_host::sync_host::PatchBox>),
    RenameLabel(Box<intel_host::RenameBox>),
    QueryRows(Box<csv_query_host::QueryBox>),
    Disassemble(Box<disasm_host::DisasmBox>),
    NewImage(Box<image_browser::NewImageBox>),
    OpenUrl(Box<web_host::WebBox>),
    WebProject(Box<project_import_host::WebProjectBox>),
    AmigaNdk(Box<ndk_host::NdkBox>),
    Toolbar(Box<toolbar_host::ToolbarBox>),
    AmigaProject(Box<amiga_project_host::NewProjectBox>),
    Database(Box<database_host::ConnectionBox>),
    AmigaTools(Box<amiga_tools_host::AmigaToolsBox>),
    AmigaIcon(Box<amiga_icon_host::IconBox>),
    StTools(Box<st_tools_host::StToolsBox>),
    C64Tools(Box<c64_music_host::C64ToolsBox>),
    LanguageServers(Vec<crate::lsp::ServerSetting>),
    Tasks(Vec<crate::tasks::Task>),
    Spelling { found: crate::spelling::Found, chosen: usize },
    AssistReview(assistant::Review),
}

pub struct Limoniter {
    /// A check for a newer OpticOS ROM under way, and whether the day's has been started.
    opticos_check: Option<opticos_host::OpticosCheck>,
    update_check: Option<update_host::UpdateCheck>,
    update_started: bool,
    /// Window, Layouts: named arrangements of the panels.
    layouts: Vec<crate::layouts::Layout>,
    /// When open files were last looked at for changes by other programs.
    last_watch: Option<std::time::Instant>,
    /// A newer version found while something else was open, told about once that's closed.
    update_waiting: Option<update_host::Offer>,
    /// A newer version being downloaded, to update to.
    update_download: Option<update_host::Download>,
    opticos_started: bool,
    docs: Vec<Document>,
    active: usize,
    untitled_count: usize,
    /// What File, New as offers.
    new_kinds: Vec<new_kinds::NewKind>,
    /// How files are compared by name, as Settings has it; each folder comparison starts with these.
    compare_rules: crate::compare_rules::Rules,
    /// Folder comparisons saved outside any project, and the file they're kept in.
    compare_sessions: Vec<crate::compare_session::Session>,
    compare_sessions_file: Option<PathBuf>,
    /// The document and version waiting to be assembled quietly, and since when.
    intel_wait: Option<(u64, u64, std::time::Instant)>,
    /// The character and sprite editor, while it's open.
    pixel_editor: Option<Box<crate::pixel_editor::Editor>>,
    tile_map: Option<Box<crate::tile_map_editor::Editor>>,
    /// Merges: the one a run from the command line is waiting on, and the offer for files with conflict markers.
    merges: merge_host::MergeState,
    /// The Spectrum screen designer, while it's open.
    screen_designer: Option<Box<crate::screen_designer::Designer>>,
    amiga_hw: Option<Box<amiga_hw_host::HwWindow>>,
    /// The Amiga bitplane converter, while it's open.
    bitplanes: Option<Box<crate::bitplane_tool::Converter>>,
    /// The Atari ST picture converter, while it's open.
    st_pictures: Option<Box<crate::st_converter::Converter>>,
    c64_pictures: Option<Box<crate::c64_converter::Converter>>,
    /// The 8-bit picture converter (Spectrum, Next, CPC, MSX, BBC), while it's open.
    retro_pictures: Option<Box<crate::retro_converter::Converter>>,
    /// The Spectrum Next's Layer 2 painter, while it's open.
    next_painter: Option<Box<crate::next_painter::Painter>>,
    atari2600_graphics: Option<Box<crate::atari2600_painter::Editor>>,
    cart_view: Option<Box<fantasy_carts_host::CartView>>,
    /// The AutoDoc reader, while it's open.
    autodoc_reader: Option<Box<autodocs_host::Reader>>,
    sound_effects: Option<Box<crate::sfx_designer::Editor>>,
    /// Where the last program built sits in its machine's memory, for the bar over the Output panel.
    memory_map: Option<crate::memory_map::Map>,
    ripper: Option<Box<crate::ripper_ui::Ripper>>,
    boot_disk: Option<Box<crate::amiga_bootdisk_ui::BootDisk>>,
    /// A folder project's index of files, being made in the background.
    indexing: workspace_host::Indexing,
    go_to_file: Option<workspace_host::GoToFile>,
    symbols: workspace_host::Symbols,
    /// What .editorconfig files say of the file in front, and the file they were read for.
    editor_config: Option<(PathBuf, crate::editorconfig::Settings)>,
    /// The blitter minterm calculator and the table generator, while they're open.
    demo_tools: demo_tools_host::DemoTools,
    /// The Atari ST hardware guide and the blitter and timer calculators, while they're open.
    st_tools: st_hw_host::StTools,
    c64_tools: c64_tools_host::C64Tools,
    c64_hw: c64_hw_host::C64Tools,
    /// Language servers: whether they're used, which, and those running.
    lsp: lsp_host::LspState,
    /// The terminal panel's shell, while it's open.
    terminal: Option<Box<crate::terminal::Terminal>>,
    /// The other terminals, behind the one in front, each with its number; and the front one's.
    terminals_behind: Vec<(u32, Box<crate::terminal::Terminal>)>,
    terminal_number: u32,
    terminals_made: u32,
    /// The Terminal panel put away, its shells still running.
    terminal_hidden: bool,
    /// A task running, and the last test task's results.
    task_run: Option<tasks_host::TaskRun>,
    test_panel: Option<tasks_host::TestPanel>,
    /// Docker Compose: the Containers panel and the logs being followed.
    compose: compose_host::ComposeState,
    tests_ui: tests_host::Explorer,
    outdated_ui: outdated_host::State,
    http_ui: http_host::State,
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
    /// The ribbon's tab and what's shown of it, when it's on in place of the menus.
    ribbon: ribbon_bar::RibbonState,
    /// What the start page has open and remembers while it's shown.
    start: start_page::StartState,
    /// The recent lists' tabs, and the names of the recent projects.
    recent_lists: recent::RecentLists,
    /// Which project each recent file was last opened or saved in. Kept in the settings beside the recent files.
    recent_in: recent::FileProjects,
    /// Chosen in the command box, to be done in the next frame.
    pending_command: Option<Command>,
    /// Drawing a menu laid open, rather than one opened by a click.
    drawing_open: bool,
    view: ViewOptions,
    show_file_tabs: bool,
    /// Tabs as a list down the side, rather than a row along the top.
    side_tabs: side_tabs::SideTabs,
    /// The row of buttons under the menus, and which buttons.
    toolbar: toolbar_host::Toolbar,
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
    /// The tab that was in front and whether the window had the focus, last frame: autosave's modes that save when
    /// either changes look at what they were.
    autosave_seen: (Option<u64>, bool),
    /// Whether the keyboard focus was in an editor the last time it was looked at, to tell when it leaves for a
    /// panel (the Explorer, Search, Output, a terminal, a dialog's box).
    autosave_in_editor: bool,
    /// Set when closing left unsaved files for the next start to bring back, rather than asking about them.
    hot_exit_kept: bool,
    /// Where the recovery copies are kept, once recovery has started.
    recovery_dir: Option<PathBuf>,
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
    /// The extensions' generation and the grammars last followed, so a change to either is taken in.
    extensions_seen: (u64, Option<&'static highlight::Engine>),
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
    /// A module or sound file playing, and why the last one couldn't be.
    music: Option<music_host::Music>,
    music_note: Option<String>,
    /// Music being saved as a WAV, in the background: what's said when it's done.
    music_saving: Option<std::sync::mpsc::Receiver<String>>,
    /// Whether a document (by id and version) is a module rather than a sound file, worked out once.
    music_is_module: Option<(u64, u64, bool)>,
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
    /// When open remote files were last looked at on their servers, and the path of one being opened by address.
    remote_watched: Option<std::time::Instant>,
    remote_opening: Option<String>,
    remote_secrets: std::collections::HashMap<String, crate::remote::Secret>,
    /// Where copies of remote files are kept; None in a test, which uses a temporary folder.
    remote_root: Option<PathBuf>,
    /// The project, when it's a folder on a server: its mirror, and the work with its server.
    remote_project: Option<remote_project_host::RemoteProject>,
    /// The themes there are, which are used, and the rules that pick a document's.
    themes: theme_host::Themes,
    /// Devices files are sent to, and sends under way.
    sends: deploy_host::Sends,
    running: run_host::Running,
    /// The PHP tools: their preview, php -l on save, and PHP's web server.
    php: php_host::PhpState,
    /// Database connections, open and kept, and the one chosen for each SQL file.
    database: database_host::DatabaseState,
    /// Python environments, and the Python and R consoles.
    python: python_host::PythonState,
    /// Programs F9 built being crunched in the background.
    amiga_jobs: Vec<amiga_tools_host::Job>,
    /// Programs and scripts running in vamos, their output coming into the Output panel.
    vamos_runs: Vec<crate::amiga_vamos::Running>,
    c64_jobs: Vec<c64_music_host::Job>,
    git: git_host::GitState,
    /// The Amiga's pens for ANSI previews, and the table that changes them.
    amiga_palette: crate::ansi_art::AmigaPalette,
    /// The keys the built-in machines' joysticks and buttons are on, and the window that changes them.
    control_keys: crate::control_hints::ControlKeys,
    control_editor: Option<crate::control_hints::Editor>,
    palette_editor: crate::amiga_palette_ui::PaletteEditor,
    /// A send waiting for the add-ons' on_send functions.
    send_pending: Option<deploy_host::PendingSend>,
    script_output: Vec<String>,
    /// Node projects: the dev server, js-debug's download, and the .env file checked against git.
    node: node_host::NodeState,
    /// Where files' histories are kept between runs. None means they aren't, as in a test.
    history_root: Option<PathBuf>,
    history_job: Option<std::thread::JoinHandle<()>>,
    /// Listening for files opened from Explorer, and any that arrived while a script was running.
    listening: Option<crate::instance::Listening>,
    handed_over: Vec<PathBuf>,
    /// Documents by when they were last in front, latest first, and a Ctrl+Tab switch that's still going.
    used_order: Vec<u64>,
    beside: Option<Beside>,
    search: search_panel::SearchPanel,
    outline: outline::Outline,
    peek: Option<intel_host::Peek>,
    /// A program being debugged; how each file's is debugged; and the box asking that.
    debugging: Option<Box<dap_host::Debugging>>,
    /// How files outside a project are debugged, by their whole path, kept in the settings file. A project's own
    /// files keep theirs in the project (`Project::debug`).
    debug_configs: crate::dap::Kept,
    debug_setup: Option<(PathBuf, crate::dap::Config)>,
    /// The chooser of VS Code's debug setups, and the bar offering the extensions its folder recommends.
    vscode: vscode_host::State,
    dragon_disk: Option<Box<dragon_disk_host::DragonDisk>>,
    binary_tools: Option<Box<binary_tools_host::BinaryTools>>,
    regex_tester: Option<Box<crate::regex_tester::Tester>>,
    code_picture: Option<Box<binary_tools_host::CodePicture>>,
    /// How AVR and PIC chips are flashed, and whether that box is open.
    flashing: crate::tasks::Flashing,
    flashing_open: bool,
    /// The serial terminal's choices, and its window while it's open.
    serial_prefs: crate::serial_link::Prefs,
    serial: Option<Box<serial_host::SerialWindow>>,
    /// The EPROM programmer's choices, and its window while it's open.
    eprom_prefs: crate::eprom::Prefs,
    eprom: Option<Box<eprom_host::EpromWindow>>,
    /// The formatter chosen for each language, and whether that box is open.
    formatter_choices: std::collections::HashMap<String, String>,
    formatters_open: bool,
    /// The folder served for a browser, reloading its pages as files are saved.
    live_server: Option<crate::live_server::Server>,
    /// Time spent working, kept if switched on; and whether its window is open, and for how many days.
    time_spent: crate::time_spent::Tracker,
    time_spent_open: Option<i64>,
    /// The Colour as box; how it was last grouped; and the languages chosen lately, newest first.
    language_picker: Option<Box<language_picker::Picker>>,
    language_view: language_picker::View,
    recent_languages: Vec<String>,
    /// The kinds last chosen in File, New as, by name, the latest first: offered at the top of the menu.
    recent_new_kinds: Vec<String>,
    /// How wide the command box (F1) was last left.
    palette_width: f32,
    /// What the status bar shows, and in what order.
    status_layout: status_items::Layout,
    /// A note of the keys for a command just chosen from a menu or the command box, and when it was shown; and how
    /// often each command's has been, so it's only now and then.
    key_note: Option<(String, std::time::Instant)>,
    key_notes_given: std::collections::HashMap<String, u8>,
    /// The command waiting was chosen in the command box.
    dialog_was_palette: bool,
    /// The window's size, as a share of the usual (the text's own size kept apart).
    window_scale: f32,
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
    /// For a file of code, the text sent to the spell checker for each line, to turn what it finds back into places.
    spell_masks: std::collections::HashMap<usize, crate::code_spelling::Masked>,
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
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>, screenshot: Option<PathBuf>, launch: Option<ToolLaunch>) -> Self {
        // System dialogs opened from now on sit over this window, and can't fall behind it.
        crate::sys_dialogs::remember_window(cc);
        let settings = read_settings_file();
        // VS Code extensions first: they give themes, grammars and icons to what follows.
        crate::vsix::start(&settings);
        theme::set_window(theme_host::Themes::from_settings(&settings, Prefs::from_settings(&settings).light).window_theme());
        theme::apply(&cc.egui_ctx);
        theme::colour_title_bar(cc);
        #[cfg(target_os = "macos")]
        crate::mac_open::set_window(cc.egui_ctx.clone());
        highlight::start_loading(cc.egui_ctx.clone());
        let font_file = list_setting(&settings, "font_file").pop().map(PathBuf::from).filter(|path| path.exists());
        if let Ok(mut window) = WINDOW_FONT.lock() {
            *window = list_setting(&settings, "window_font").pop().map(PathBuf::from).filter(|path| path.exists());
        }
        install_fonts(&cc.egui_ctx, font_file.as_deref());
        let mut app = Self::build(settings, font_file, screenshot);
        for file in &files {
            app.open_path(file);
        }
        if app.docs.is_empty() {
            app.new_document();
        }
        // A window for one diff or merge, as Git asks for, doesn't take files opened from elsewhere.
        let tool = launch.is_some();
        if let Some(launch) = launch {
            app.waker = Some(cc.egui_ctx.clone());
            app.start_tool(launch);
        }
        if app.screenshot.is_none() {
            app.start_recovery();
        }
        if !tool {
            app.start_listening(&cc.egui_ctx);
        }
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
        crate::i18n::set(prefs.language);
        crate::asm::m68k::choose(prefs.m68k_processor);
        crate::asm::m68k::choose_fpu(prefs.m68k_fpu);
        crate::asm::m68k::choose_mmu(prefs.m68k_mmu);
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
        let compare_sessions_file = script_home.as_ref().map(|home| home.join(crate::compare_session::FILE_NAME));
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
            remote_watched: None,
            remote_opening: None,
            remote_secrets: std::collections::HashMap::new(),
            remote_root,
            remote_project: None,
            sends: deploy_host::Sends::from_settings(&settings),
            running: run_host::Running::from_settings(&settings),
            php: php_host::PhpState::from_settings(&settings),
            database: database_host::DatabaseState::from_settings(&settings),
            python: python_host::PythonState::from_settings(&settings),
            amiga_jobs: Vec::new(),
            vamos_runs: Vec::new(),
            c64_jobs: Vec::new(),
            git: git_host::GitState::from_settings(&settings),
            amiga_palette: crate::ansi_art::AmigaPalette::from_settings(&settings),
            control_keys: crate::control_hints::ControlKeys::from_settings(&settings),
            control_editor: None,
            palette_editor: Default::default(),
            themes: theme_host::Themes::from_settings(&settings, Prefs::from_settings(&settings).light),
            send_pending: None,
            script_output: Vec::new(),
            node: Default::default(),
            history_root,
            history_job: None,
            listening: None,
            handed_over: Vec::new(),
            used_order: Vec::new(),
            beside: None,
            search: Default::default(),
            outline: Default::default(),
            peek: None,
            debugging: None,
            debug_configs: crate::dap::kept_from_settings(&settings),
            debug_setup: None,
            vscode: Default::default(),
            dragon_disk: None,
            binary_tools: None,
            regex_tester: None,
            code_picture: None,
            flashing: {
                let mut flashing = crate::tasks::Flashing::default();
                for line in settings.lines() {
                    if let Some((name, value)) = line.split_once('=') {
                        flashing.read_setting(name.trim(), value);
                    }
                }
                flashing
            },
            flashing_open: false,
            serial_prefs: {
                let mut prefs = crate::serial_link::Prefs::default();
                for line in settings.lines() {
                    if let Some((name, value)) = line.split_once('=') {
                        prefs.read_setting(name.trim(), value);
                    }
                }
                prefs
            },
            serial: None,
            eprom_prefs: {
                let mut prefs = crate::eprom::Prefs::default();
                for line in settings.lines() {
                    if let Some((name, value)) = line.split_once('=') {
                        prefs.read_setting(name.trim(), value);
                    }
                }
                prefs
            },
            eprom: None,
            formatter_choices: {
                let mut choices = std::collections::HashMap::new();
                for value in list_setting(&settings, "formatter") {
                    crate::ext_formatters::read_setting(&value, &mut choices);
                }
                choices
            },
            formatters_open: false,
            live_server: None,
            time_spent: crate::time_spent::Tracker::load(if cfg!(test) { None } else { settings::data_folder().map(|d| d.join("time-spent.tsv")) }),
            time_spent_open: None,
            language_picker: None,
            language_view: language_picker::View::from_setting(list_setting(&settings, "language_view").first().map(String::as_str)),
            recent_languages: list_setting(&settings, "recent_language"),
            recent_new_kinds: list_setting(&settings, "recent_new_kind"),
            key_note: None,
            key_notes_given: std::collections::HashMap::new(),
            dialog_was_palette: false,
            status_layout: {
                let mut layout = status_items::Layout::default();
                for name in ["status_order", "status_hidden"] {
                    if let Some(value) = list_setting(&settings, name).first() {
                        layout.read_setting(name, value);
                    }
                }
                layout
            },
            palette_width: list_setting(&settings, "palette_width").first().and_then(|w| w.parse().ok()).unwrap_or(560.0),
            window_scale: list_setting(&settings, "window_scale").first().and_then(|w| w.parse::<f32>().ok()).filter(|s| WINDOW_SCALES.contains(s)).unwrap_or(1.0),
            cycle: None,
            hold_cycle: false,
            snippets: file_tools::snippets_from_settings(&settings),
            run_job: None,
            run_history: list_setting(&settings, "run_command"),
            split_job: None,
            speller: None,
            spell_problem: None,
            spell_asked: false,
            spell_masks: std::collections::HashMap::new(),
            waker: None,
            region: None,
            tags_kept: std::collections::HashMap::new(),
            assistant: assistant::Setup::from_settings(&settings),
            assist_job: None,
            assist_panel: assistant::AssistPanel::default(),
            assist_log,
            opticos_check: None,
            update_check: None,
            update_started: false,
            layouts: crate::layouts::from_settings(&settings),
            last_watch: None,
            update_waiting: None,
            update_download: None,
            opticos_started: false,
            docs: Vec::new(),
            active: 0,
            untitled_count: 0,
            new_kinds: new_kinds::kinds_from_settings(&settings),
            compare_rules: crate::compare_rules::Rules::from_settings(&settings),
            compare_sessions: compare_sessions_file.as_deref().map_or_else(Vec::new, crate::compare_session::load),
            compare_sessions_file,
            intel_wait: None,
            pixel_editor: None,
            tile_map: None,
            merges: merge_host::MergeState::default(),
            screen_designer: None,
            amiga_hw: None,
            bitplanes: None,
            st_pictures: None,
            c64_pictures: None,
            retro_pictures: None,
            next_painter: None,
            atari2600_graphics: None,
            cart_view: None,
            autodoc_reader: None,
            sound_effects: None,
            memory_map: None,
            ripper: None,
            boot_disk: None,
            indexing: Default::default(),
            go_to_file: None,
            symbols: Default::default(),
            editor_config: None,
            demo_tools: Default::default(),
            st_tools: Default::default(),
            c64_tools: Default::default(),
            c64_hw: Default::default(),
            lsp: lsp_host::LspState::from_settings(&settings),
            terminal: None,
            terminals_behind: Vec::new(),
            terminal_number: 0,
            terminals_made: 0,
            terminal_hidden: false,
            task_run: None,
            test_panel: None,
            compose: compose_host::ComposeState::default(),
            tests_ui: Default::default(),
            outdated_ui: Default::default(),
            http_ui: http_host::State::from_settings(&settings),
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
            ribbon: ribbon_bar::RibbonState::default(),
            start: start_page::StartState::default(),
            recent_lists: recent::RecentLists::default(),
            recent_in: recent::FileProjects::from_settings(&settings),
            drawing_open: false,
            view,
            show_file_tabs: setting_flag(&settings, "file_tabs", true),
            side_tabs: side_tabs::SideTabs::from_settings(&settings),
            toolbar: toolbar_host::Toolbar::from_settings(&settings),
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
            autosave_seen: (None, true),
            autosave_in_editor: false,
            hot_exit_kept: false,
            recovery_dir: None,
            recovery_failed: None,
            jumps: JumpHistory::default(),
            font_file,
            open_request: None,
            fill_skip_last: true,
            prefs,
            applied_window: None,
            extensions_seen: (crate::vsix::generation(), None),
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
            music: None,
            music_note: None,
            music_saving: None,
            music_is_module: None,
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
            self.recovery_failed = Some(crate::i18n::tr("There's nowhere to keep them").to_string());
            return;
        };
        self.start_recovery_in(root);
    }

    fn start_recovery_in(&mut self, root: PathBuf) {
        // A last quit that left its unsaved files for now says so with a marker, which is only good for one start.
        self.recovery_dir = Some(root.clone());
        let marker = root.join(HOT_EXIT_MARKER);
        let hot = self.prefs.hot_exit && marker.exists();
        let _ = std::fs::remove_file(&marker);
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
        if hot {
            self.bring_back_after_hot_exit();
        }
    }

    /// After a quit that kept unsaved files: they come straight back as they were, with no question, unless one of
    /// them can't be read or its file has been changed by something else meanwhile, when the usual offer is made.
    fn bring_back_after_hot_exit(&mut self) {
        let Dialog::Recovery(offer) = &self.dialog else { return };
        if offer.items.iter().any(|item| item.content.is_none() || item.changed_on_disk) {
            return;
        }
        self.recover_selected();
        self.print_status = crate::i18n::tr("Brought back the files you hadn't saved").to_string();
    }

    /// Keeps a file with unsaved changes for the next start instead of asking about it, when hot exit is on.
    /// Says whether it was kept: a copy has to be safely on the disk first.
    fn keep_for_next_start(&mut self, index: usize) -> bool {
        let doc = &self.docs[index];
        if !self.prefs.hot_exit || doc.scanning.is_some() || doc.compare.is_some() || doc.saving.is_some() {
            return false;
        }
        let kept = self.recovery.as_mut().is_some_and(|session| session.write_now(&doc.snapshot()).is_ok());
        self.hot_exit_kept |= kept;
        kept
    }

    /// The window is closing: every file with unsaved changes is kept or asked about. False if the person cancels.
    fn settle_unsaved_for_quit(&mut self) -> bool {
        self.hot_exit_kept = false;
        for index in 0..self.docs.len() {
            if !self.docs[index].is_modified() || self.keep_for_next_start(index) {
                continue;
            }
            if !self.confirm_close(index) {
                return false;
            }
            // Answered No: whatever copy was being kept of it isn't wanted.
            let id = self.docs[index].id;
            if let Some(session) = self.recovery.as_mut() {
                session.remove(id);
            }
        }
        true
    }

    /// Closing for good: the recovery copies go, unless hot exit is leaving some for the next start.
    fn finish_recovery(&mut self) {
        if let Some(session) = self.recovery.take() {
            if self.hot_exit_kept && self.recovery_dir.as_ref().is_some_and(|root| std::fs::write(root.join(HOT_EXIT_MARKER), b"").is_ok()) {
                session.abandon();
            } else {
                session.finish();
            }
        }
    }

    fn offer_recovered(&mut self, root: &Path, asked: bool) {
        let (orphans, found) = recovery::find_orphans(root);
        if found.is_empty() {
            recovery::tidy(orphans);
            if asked {
                self.message("Recover edits", crate::i18n::tr("There are no unsaved edits waiting to be recovered.").into());
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
        crate::fit::Dialog::new(crate::i18n::tr("Recover unsaved edits")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_width(620.0);
            ui.label(crate::i18n::tr("Limoniter didn't close normally last time. These unsaved edits were kept."));
            ui.label(RichText::new(crate::i18n::tr("Recovering opens them as unsaved changes. The files on disk aren't touched until you save.")).size(12.5).color(theme::text_dim()));
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
                recover = ui.button(crate::i18n::tr("Recover selected")).clicked();
                later = ui.button(crate::i18n::tr("Decide later")).on_hover_text(crate::i18n::tr("Keeps them. They're offered again next time, or from File, Recover unsaved edits")).clicked();
                discard = ui.button(crate::i18n::tr("Discard selected...")).clicked();
            });
        });
        if later || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.finish_offer();
        } else if discard {
            let count = offer.items.iter().filter(|item| item.selected || item.content.is_none()).count();
            if ask_yes_no("Recover unsaved edits", &if count == 1 { crate::i18n::tr("Permanently discard 1 recovery copy?").to_string() } else { crate::i18n::tf!("Permanently discard {count} recovery copies?") }) {
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
                Err(problem) => warn(&crate::i18n::tf!("Couldn't recover {title}.\n\n{problem}\n\nThe copy has been kept at {}.", file.display())),
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
        let preset = self.new_file_preset_now();
        self.docs.push(Document::untitled(format!("Edit{}", self.untitled_count), preset));
        self.active = self.docs.len() - 1;
        self.pending_focus = true;
    }

    fn open_dialog(&mut self) {
        if let Some(files) = with_open_filters(crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Open"))).pick_files() {
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

    /// An AMOS or STOS program, which opens with its listing beside it: its bytes mean nothing in hex alone.
    fn amos_or_stos(doc: &Document) -> bool {
        doc.hex_mode && {
            let head = doc.hex.read(0, 16);
            crate::amos::looks_like(&[head.as_slice(), &[0; 8]].concat()) || crate::stos::looks_like(&[head.as_slice(), &[0; 80]].concat())
        }
    }

    /// The preview a document gets: a picture if its bytes are one, otherwise what its name and reading say.
    fn preview_kind(doc: &Document) -> Option<preview::Kind> {
        if doc.picture.is_some() {
            return Some(preview::Kind::Picture);
        }
        if doc.legacy.is_some() {
            return Some(preview::Kind::Document);
        }
        if doc.hex_mode && (crate::protracker::has_module_tag(&doc.hex.read(0, 1084)) || crate::sid::looks_like(&doc.hex.read(0, 0x7C))) {
            return Some(preview::Kind::Module);
        }
        if doc.hex_mode && crate::st_music::recognised(&doc.hex.read(0, 64), &doc.name()) {
            return Some(preview::Kind::Module);
        }
        if doc.hex_mode && crate::tracker::pc_format(&doc.hex.read(0, 64)).is_some() {
            return Some(preview::Kind::Module);
        }
        if doc.hex_mode && crate::sound::recognised(&doc.hex.read(0, 64), &doc.name()) {
            return Some(preview::Kind::Sound);
        }
        preview::kind_for_file(Some(&doc.path_or_name()), &doc.language_name(), &doc.text.format)
    }

    fn open_path(&mut self, path: &Path) -> bool {
        // A file on a server, by its address.
        if let Some(address) = path.to_str().filter(|p| crate::remote::is_address(p)) {
            return self.open_sftp_address(address);
        }
        // A file of a remote project left on its server: fetched, then opened.
        if self.remote_project_stand_in(path) {
            return true;
        }
        // VS Code's workspaces open as a project.
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("code-workspace")) && path.is_file() {
            return self.open_code_workspace(path);
        }
        let full = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(i) = self.docs.iter().position(|d| d.path.as_deref().is_some_and(|p| same_path(p, &full))) {
            self.active = i;
            self.pending_focus = true;
            self.note_recent_file(&full);
            return true;
        }
        // Spreadsheets and JSON-lines files open in the data viewer.
        if data_view_host::is_data_name(&full.to_string_lossy()) && self.encoding_prefs.get(&full).is_none() {
            return self.open_data_file(&full);
        }
        // Disk, tape and CD images and archives open in a browser, unless it turns out they can't be read as one.
        if self.encoding_prefs.get(&full).is_none() && self.open_image(&full) {
            self.pending_focus = true;
            return true;
        }
        // A file packed with Pack-Ice or Atomik, or crunched on the Amiga, opens as what it holds, unless it's been
        // given an encoding.
        if self.encoding_prefs.get(&full).is_none() && self.open_unpacked(&full) {
            return true;
        }
        match Document::open(&full, self.encoding_prefs.get(&full)) {
            Ok(mut doc) => {
                self.note_recent_file(&full);
                // A file Windows has marked read-only opens locked; the switch on the Edit tab unlocks it.
                if std::fs::metadata(&full).is_ok_and(|meta| meta.permissions().readonly()) {
                    doc.set_read_only(true);
                }
                doc.preview_view = self.opening_view(Some(&full), doc.picture.is_some() || matches!(Self::preview_kind(&doc), Some(preview::Kind::Module | preview::Kind::Sound | preview::Kind::Document)) || Self::amos_or_stos(&doc));
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
                warn(&crate::i18n::tf!("Couldn't open {}.\n\n{e}", full.display()));
                false
            }
        }
    }

    /// Opens a file packed with an Atari ST packer as what it holds, read-only, named for the file and its packer.
    /// False if it isn't packed with one Limoniter knows, or doesn't unpack, so it opens as it is.
    fn open_unpacked(&mut self, path: &Path) -> bool {
        let mut head = [0u8; 12];
        let len = std::fs::metadata(path).map_or(0, |m| m.len());
        if len > 64 << 20 || std::fs::File::open(path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_err() {
            return false;
        }
        if crate::amiga_crunch::worth_reading(&head, len) {
            return self.open_decrunched(path);
        }
        if crate::st_packers::packer(&head).is_none() {
            return false;
        }
        let Some(Ok(unpacked)) = std::fs::read(path).ok().and_then(|bytes| crate::st_packers::unpack(&bytes)) else {
            return false;
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.print_status = crate::i18n::tf!("{name} is packed with {} ({} bytes): it's open as what it holds, {} bytes, read-only.", unpacked.packer.name(), unpacked.packed_size, unpacked.bytes.len());
        let mut doc = Document::from_image_file(format!("{} \u{203A} {name}", unpacked.packer.name()), unpacked.bytes, None);
        doc.preview_view = self.opening_view(Some(Path::new(&name)), doc.picture.is_some() || matches!(Self::preview_kind(&doc), Some(preview::Kind::Module | preview::Kind::Sound | preview::Kind::Document)) || Self::amos_or_stos(&doc));
        self.note_recent_file(path);
        if self.docs.len() == 1 && self.docs[0].is_blank() {
            self.docs.clear();
        }
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
        self.pending_focus = true;
        true
    }

    /// Opens a file crunched on the Amiga. Crunched data opens as what it holds, read-only, named for the file and
    /// its cruncher; a crunched program opens as it is (it runs as it is), with a word on what it is and what can be
    /// done with it. False if it isn't crunched with anything Limoniter knows, so it opens as it is.
    fn open_decrunched(&mut self, path: &Path) -> bool {
        let Ok(bytes) = std::fs::read(path) else {
            return false;
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(found) = crate::amiga_crunch::recognise(&bytes) else {
            return false;
        };
        if found.program {
            self.print_status = crate::i18n::tf!("{name}: {}; Disassemble shows it decrunched.", crate::amiga_crunch::said(&found));
            return false;
        }
        let decrunched = match crate::amiga_crunch::decrunch(&bytes) {
            Some(Ok(decrunched)) => decrunched,
            Some(Err(why)) => {
                self.print_status = why;
                return false;
            }
            None => return false,
        };
        let cruncher = decrunched.found.name.clone();
        self.print_status = crate::i18n::tf!("{name} was crunched with {cruncher} ({} bytes): it's open as what it holds, {} bytes, read-only.", bytes.len(), decrunched.bytes.len());
        let mut doc = Document::from_image_file(format!("{cruncher} \u{203A} {name}"), decrunched.bytes, None);
        doc.preview_view = self.opening_view(Some(Path::new(&name)), doc.picture.is_some() || matches!(Self::preview_kind(&doc), Some(preview::Kind::Module | preview::Kind::Sound | preview::Kind::Document)) || Self::amos_or_stos(&doc));
        self.note_recent_file(path);
        if self.docs.len() == 1 && self.docs[0].is_blank() {
            self.docs.clear();
        }
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
        self.pending_focus = true;
        true
    }

    /// Saves what the file in front decrunches to (an Amiga file crunched with PowerPacker, Imploder, StoneCracker,
    /// CrunchMania or ByteKiller): the data, or the program built again. With nothing crunched open, a crunched
    /// file is asked for.
    fn save_decrunched(&mut self) {
        let crunched = (!self.no_file()).then(|| self.document_bytes()).filter(|bytes| crate::amiga_crunch::recognise(bytes).is_some());
        let (bytes, name) = match crunched {
            Some(bytes) => (bytes, self.docs[self.active].path_or_name().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            None => {
                let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("A crunched Amiga file to decrunch")).pick_file() else {
                    return;
                };
                match std::fs::read(&path) {
                    Ok(bytes) => (bytes, path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                    Err(e) => return warn(&crate::i18n::tf!("Couldn't open {}.\n\n{e}", path.display())),
                }
            }
        };
        let decrunched = match crate::amiga_crunch::decrunch(&bytes) {
            None => return self.message("Save the decrunched program", crate::i18n::tf!("{name} isn't crunched with PowerPacker, Imploder, StoneCracker, CrunchMania or ByteKiller, as far as Limoniter can tell.")),
            Some(Err(why)) => return self.message("Save the decrunched program", why),
            Some(Ok(decrunched)) => decrunched,
        };
        // A data file loses the ending its cruncher gave it, if it has one.
        let suggested = ["pp", "imp", "im", "crm", "stc", "bk"].iter().find_map(|ending| name.strip_suffix(&format!(".{ending}"))).unwrap_or(&name).to_string();
        let title = if decrunched.found.program { crate::i18n::tr("Save the decrunched program") } else { crate::i18n::tr("Save the decrunched file") };
        let Some(target) = crate::sys_dialogs::file_dialog().set_title(title).set_file_name(&suggested).save_file() else {
            return;
        };
        match std::fs::write(&target, &decrunched.bytes) {
            Ok(()) => self.print_status = crate::i18n::tf!("{}; saved decrunched as {} ({} bytes).", decrunched.said(), target.display(), decrunched.bytes.len()),
            Err(e) => warn(&crate::i18n::tf!("Couldn't save {}.\n\n{e}", target.display())),
        }
    }

    /// The bytes to save or show in hex. Anything that won't fit is listed for the person,
    /// and nothing is written or changed until they've dealt with it.
    fn bytes_for(&mut self, index: usize) -> Option<Vec<u8>> {
        let doc = &mut self.docs[index];
        if doc.hex_mode {
            return Some(doc.hex.bytes().to_vec());
        }
        // What .editorconfig says of saving it: trailing spaces off, and a line ending at the end, or not.
        let config = doc.path.as_deref().map(crate::editorconfig::for_file).unwrap_or_default();
        // A Markdown file's table of contents is kept up to date as it's saved.
        if doc.text.markdown && !doc.text.is_disk_backed() {
            let lines = doc.text.all_lines();
            if let Some((start, end, rows)) = crate::markdown_tools::refresh_toc(&lines)
                && rows.iter().map(String::as_str).ne(lines[start..=end].iter().map(|l| &**l))
            {
                // Kept up to date without moving the cursor: replacing selects what's put in.
                let (line, column) = doc.text.caret_char_position();
                doc.text.replace_lines(start, end, rows);
                doc.text.move_caret_to_char(line, column);
            }
        }
        // .editorconfig's word goes first; then the person's own setting.
        // .editorconfig's trim_trailing_whitespace means the whole file; only the person's own setting can keep to
        // the lines changed.
        let project = self.project.as_ref().map(|p| p.eff().into_owned());
        let project = project.as_ref();
        if config.trim_trailing_whitespace.or(project.and_then(|p| p.trim_trailing)).unwrap_or(self.view.trim_on_save) && !doc.text.is_disk_backed() {
            doc.text.trim_trailing_on_save(config.trim_trailing_whitespace.is_none() && project.is_none_or(|p| p.trim_trailing.is_none()) && self.view.trim_only_changed);
        }
        if config.insert_final_newline.or(project.and_then(|p| p.final_newline)).unwrap_or(doc.text.format.ensure_final_newline) {
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
        if self.docs[index].data.is_some() {
            return self.save_data_csv(index);
        }
        if self.docs[index].game.is_some() {
            return false;
        }
        if self.docs[index].compare.is_some() {
            return self.save_compare_tab(index);
        }
        if self.docs[index].browser.is_some() {
            self.message("Save", crate::i18n::tr("An image or archive is only read here, so nothing is saved over it. Save a copy takes a file out of it.").into());
            return false;
        }
        if self.docs[index].merge.is_some() {
            return self.save_merge(index);
        }
        match self.docs[index].path.clone() {
            Some(path) => {
                if self.format_on_save_now() {
                    self.lsp_format(index, true);
                }
                let written = self.write_to(index, &path);
                // Pages served by the live server reload.
                if written && let Some(server) = &self.live_server {
                    server.saved();
                }
                written
            }
            None => self.save_as(index),
        }
    }

    fn save_as(&mut self, index: usize) -> bool {
        if self.docs[index].data.is_some() {
            return self.save_data_csv(index);
        }
        if self.docs[index].game.is_some() || self.docs[index].compare.is_some() {
            return false;
        }
        if self.docs[index].browser.is_some() {
            self.message("Save as", crate::i18n::tr("An image or archive is only read here. Save a copy, in the browser, takes a file out of it.").into());
            return false;
        }
        let name = self.docs[index].name();
        match crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save As")).set_file_name(name).save_file() {
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
                let question = crate::i18n::tf!("Row {} and {} other row(s) aren't the width this format needs, which would shift everything after them.\n\nSave anyway?", first + 1, uneven.len() - 1);
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
            warn(&crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()));
            return false;
        }
        self.note_recent_file(path);
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
        self.amiga_saved(path);
        self.php_saved(index, path);
        self.note_saved_in_project(path);
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
            Err(e) => warn(&crate::i18n::tf!("Couldn't keep a backup of {} before saving.\n\n{e}\n\nIt will still be saved.", path.display())),
        }
    }

    /// How long autosave waits: the settings', or VS Code's `files.autoSaveDelay` where the project follows that and
    /// has chosen its own way to autosave from it.
    fn autosave_seconds_now(&self) -> u32 {
        match self.project.as_ref() {
            Some(project) if project.autosave.is_none() && project.vscode.autosave.is_some() => project.vscode.autosave_seconds.unwrap_or(self.prefs.autosave_seconds),
            _ => self.prefs.autosave_seconds,
        }
    }

    /// What starts an autosave now, if anything does: the project's own choice, or else the settings'.
    pub(crate) fn autosave_mode(&self) -> Option<settings::AutosaveWhen> {
        match self.project.as_ref().and_then(|project| project.eff_autosave()) {
            Some(settings::AutosaveChoice::Off) => None,
            Some(settings::AutosaveChoice::When(when)) => Some(when),
            None => self.prefs.autosave.then_some(self.prefs.autosave_when),
        }
    }

    /// Saves a file with unsaved changes by itself, unless that would need a question answered, would mean saving
    /// over something else, or (if asked) the language server says it has errors. Says whether it was saved.
    fn autosave_now(&mut self, index: usize) -> bool {
        let skip_errors = self.prefs.autosave_skip_errors;
        let doc = &mut self.docs[index];
        if !doc.is_modified() || doc.is_busy() {
            return false;
        }
        if let Some(never) = autosave_never(doc) {
            doc.autosave_skipped = Some(never);
            return false;
        }
        doc.autosave_skipped = autosave_not_now(doc, skip_errors);
        if doc.autosave_skipped.is_some() {
            return false;
        }
        let path = doc.path.clone().expect("checked above");
        if self.write_to(index, &path) {
            self.print_status = crate::i18n::tf!("Saved {} by itself", self.docs[index].name());
            return true;
        }
        false
    }

    /// Saves files by themselves as the mode asks: after the delay a file has had unsaved changes for, when another
    /// tab or panel is chosen (the one left is saved), or when the window loses the focus (all of them are).
    fn autosave(&mut self, ctx: &Context) {
        let mode = self.autosave_mode();
        let active = self.docs.get(self.active).map(|doc| doc.id);
        let focused = ctx.input(|i| i.focused);
        let (left_tab, lost_window) = (self.autosave_seen.0.filter(|id| Some(*id) != active), self.autosave_seen.1 && !focused);
        self.autosave_seen = (active, focused);
        // The keyboard focus leaving the editor for a panel counts as choosing it, as VS Code's onFocusChange has
        // it. A click on nothing in particular (which lets go of the focus) counts too, but the focus going by
        // itself (the window being left, a key letting go of it) does not.
        let focus = ctx.memory(|m| m.focused());
        let clicked = ctx.input(|i| i.pointer.any_pressed());
        let in_editor = focus.is_some_and(views::is_editor_id);
        let left_editor = self.autosave_in_editor && !in_editor && focused && (focus.is_some() || clicked);
        if focus.is_some() || clicked {
            self.autosave_in_editor = in_editor;
        }
        // A screenshot never saves over a file; tests run as one.
        let Some(mode) = mode.filter(|_| !(self.screenshot.is_some() && !cfg!(test)) && self.script.is_none()) else {
            for doc in &mut self.docs {
                doc.unsaved_since = None;
                doc.autosave_skipped = None;
            }
            return;
        };
        if mode != settings::AutosaveWhen::AfterDelay {
            let mut leaving: Vec<usize> = Vec::new();
            if lost_window {
                leaving.extend(0..self.docs.len());
            } else if mode == settings::AutosaveWhen::OnFocusChange {
                if let Some(id) = left_tab
                    && let Some(index) = self.docs.iter().position(|doc| doc.id == id)
                {
                    leaving.push(index);
                }
                // The file in front, when the focus has gone to a panel.
                if left_editor && self.docs.get(self.active).is_some() && !leaving.contains(&self.active) {
                    leaving.push(self.active);
                }
            }
            for doc in &mut self.docs {
                doc.unsaved_since = None;
                if !doc.is_modified() {
                    doc.autosave_skipped = None;
                }
            }
            for index in leaving {
                self.autosave_now(index);
            }
            return;
        }
        let wait = std::time::Duration::from_secs(u64::from(self.autosave_seconds_now()));
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
            doc.autosave_skipped = autosave_not_now(doc, self.prefs.autosave_skip_errors);
            doc.unsaved_since = doc.autosave_skipped.map(|_| now);
            if doc.autosave_skipped.is_some() {
                ctx.request_repaint_after(wait);
                continue;
            }
            self.autosave_now(index);
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
            let question = crate::i18n::tf!("{} has been changed by another program since it was opened.\n\nOverwrite it?", path.display());
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
            *worker_result.lock().unwrap() = Some(attempt.unwrap_or_else(|_| Err(crate::i18n::tr("Saving failed unexpectedly.").to_string())));
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
            let question = crate::i18n::tf!("{} has been changed by another program since it was opened.\n\nOverwrite it?", path.display());
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
            *worker_result.lock().unwrap() = Some(attempt.unwrap_or_else(|_| Err(crate::i18n::tr("Saving failed unexpectedly.").to_string())));
        });
        doc.saving = Some(SaveJob { path: path.to_path_buf(), written, total, result });
        true
    }

    /// One level of indent, as the Tab key would make it.
    fn indent_unit(&self) -> String {
        if self.spaces_for_tab_now() { " ".repeat(self.tab_size_now()) } else { "\t".to_string() }
    }

    /// These commands act on whole lines, so several selections have to be ended first. Says so if they aren't.
    fn one_selection(&mut self, title: &str) -> bool {
        if self.docs[self.active].text.has_multi() {
            self.message(title, crate::i18n::tr("Press Esc to go back to one selection first.").into());
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
            self.message("Paragraph", crate::i18n::tr("Put the cursor in a paragraph, or select some lines.").into());
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
    /// Format, Convert: the selection, or the whole file, converted in place.
    fn run_conversion(&mut self, conversion: conversions::Conversion) {
        let title = conversion.title();
        if !self.one_selection(title) {
            return;
        }
        let doc = &self.docs[self.active];
        if doc.text.is_block() {
            self.message(title, crate::i18n::tr("Turn off Column Mode first, then try again.").into());
            return;
        }
        let whole = !doc.text.has_selection();
        let Some(text) = doc.text.selection_or_all() else {
            self.message(title, crate::i18n::tr("This file is too large to convert as a whole. Select the part you want instead.").into());
            return;
        };
        match conversion.apply(&text) {
            Ok(converted) => {
                let buffer = &mut self.doc().text;
                buffer.replace_selection_or_all(&converted);
                if whole {
                    buffer.move_caret(0, 0, false);
                }
            }
            Err(problem) => self.message(title, problem),
        }
    }

    fn run_formatter(&mut self, kind: formatters::Kind) {
        if !self.one_selection(kind.title()) {
            return;
        }
        let doc = &self.docs[self.active];
        if doc.text.is_block() {
            self.message(kind.title(), crate::i18n::tr("Turn off Column Mode first, then try again.").into());
            return;
        }
        let whole = !doc.text.has_selection();
        let Some(text) = doc.text.selection_or_all() else {
            self.message(kind.title(), crate::i18n::tr("This file is too large to reformat as a whole. Select the part you want instead.").into());
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
        self.recent_lists.forget(path);
        self.recent_projects.retain(|known| !same_path(known, path));
        self.recent_projects.insert(0, path.to_path_buf());
        self.recent_projects.truncate(MOST_RECENT);
    }

    /// Keeps the project's file up to date. Not being able to is worth saying.
    fn save_project(&mut self) {
        // Its name, as the recent lists show it, may have changed.
        if let Some(project) = &self.project {
            self.recent_lists.forget(&project.path);
        }
        if let Some(problem) = self.project.as_ref().and_then(|project| project.save().err().map(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", project.path.display()))) {
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
                self.start_remote_project();
                for file in files {
                    self.open_path(&file);
                }
                self.offer_to_hide_build_folders();
                self.index_project();
            }
            Err(problem) => {
                self.recent_projects.retain(|known| !same_path(known, path));
                self.message("Open project", crate::i18n::tf!("Couldn't open {}.\n\n{problem}", path.display()));
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
        self.remote_project = None;
    }

    fn add_to_project(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let paths: Vec<PathBuf> = paths.into_iter().collect();
        if let Some(project) = &mut self.project {
            project.add(paths.iter().cloned());
            self.save_project();
        }
        // An Amiga project's makefiles take in the C files added.
        for path in &paths {
            self.amiga_files_changed(path);
        }
    }

    fn backup_project(&mut self) {
        if self.project_job.is_some() {
            self.message("Backup project", crate::i18n::tr("A backup or restore is still running.").into());
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
            self.message("Restore project", crate::i18n::tr("A backup or restore is still running.").into());
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

    /// Asks what to compare this file with: another open file, or one on disk.
    fn begin_compare(&mut self) {
        let doc = &self.docs[self.active];
        if doc.is_busy() {
            self.message("Compare", crate::i18n::tr("Please wait for this file to finish being read or written first.").into());
            return;
        }
        if doc.compare.is_some() || doc.browser.is_some() {
            self.message("Compare", crate::i18n::tr("This tab isn't a file that can be compared. Compare folders and Compare files are in the Tools menu.").into());
            return;
        }
        let others = self.docs.iter().enumerate().filter(|(index, other)| *index != self.active && !other.placeholder && other.compare.is_none() && other.browser.is_none()).map(|(index, other)| (index, other.name())).collect();
        self.dialog = Dialog::ComparePick(others);
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
                self.message("Print", crate::i18n::tr("This file is too large to print in hex. It can be up to 16 MB.").into());
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
        self.watch_remote_files(ctx);
        let mut busy = self.check_big_job() | self.check_remote_jobs() | self.check_sends() | self.check_sessions();
        busy |= self.remote_project_tick(ctx);
        busy |= self.git_tick(ctx);
        if self.lsp_tick() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        self.assembly_tick();
        self.compile_tick(ctx);
        self.amiga_jobs_tick(ctx);
        self.vamos_tick(ctx);
        self.c64_jobs_tick(ctx);
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
        busy |= self.php_tick();
        busy |= self.outdated_tick();
        busy |= self.http_tick(ctx);
        self.package_script_links();
        busy |= self.xdebug_tick();
        self.database.ctx = Some(ctx.clone());
        busy |= self.database_tick();
        busy |= self.node_tick();
        busy |= self.python_tick(ctx);
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
                    self.print_status = if pages == 1 { crate::i18n::tr("Sent 1 page to the printer").to_string() } else { crate::i18n::tf!("Sent {pages} pages to the printer") };
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
                self.find.status = crate::i18n::tf!("Replacing: {} of {} files...", job.done.load(Ordering::Relaxed), job.total.load(Ordering::Relaxed));
                busy = true;
            }
        }
        for doc in &mut self.docs {
            if let Some(source) = doc.scanning.clone() {
                doc.text.refresh_from_source();
                if source.is_finished() {
                    if let Some(problem) = source.failure() {
                        warn(&crate::i18n::tf!("Couldn't finish reading {}.\n\n{problem}\n\nOnly the part read so far is shown. Saving is not safe until the file is reopened.", source.path().display()));
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
                    warn(&crate::i18n::tf!("Couldn't save {}.\n\n{problem}\n\nThe original file has not been changed.", job.path.display()));
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
            self.message(title, crate::i18n::tr("Please wait for the file being worked through to finish.").into());
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
            return self.message(title, crate::i18n::tr("Please wait for the file being worked through to finish.").into());
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
            crate::big_jobs::Outcome::Compared(comparison) => {
                self.show_streamed_compare(comparison);
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
                    Err(e) => warn(&crate::i18n::tf!("{title} couldn't read back what it wrote.\n\n{e}")),
                }
                return true;
            }
            crate::big_jobs::Outcome::Queried { table, query, delimiter, header } => {
                self.finish_query(doc_id, table, query, delimiter, header);
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
                    warn(&crate::i18n::tf!("Couldn't save {}.\n\n{problem}\n\nThe original file has not been changed.", job.path.display()));
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
        let answer = crate::sys_dialogs::message_dialog()
            .set_title(crate::i18n::tr("Limoniter"))
            .set_level(MessageLevel::Warning)
            .set_description(crate::i18n::tf!("Save changes to {}?", self.docs[index].name()))
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
        self.remove_tab(index);
    }

    /// Closes every file but `keep` (all of them with None), from the last tab back, asking about each one with
    /// unsaved changes as Close does. Cancel stops there, leaving the rest open.
    fn close_all(&mut self, keep: Option<usize>) {
        if self.no_file() {
            return;
        }
        for index in (0..self.docs.len()).rev() {
            // Only tabs after `keep` have gone so far, so its place hasn't moved.
            if Some(index) == keep {
                continue;
            }
            if !self.confirm_close(index) {
                break;
            }
            self.remove_tab(index);
        }
        if let Some(kept) = keep {
            self.active = kept.min(self.docs.len().saturating_sub(1));
        }
        self.pending_focus = true;
    }

    /// Takes a document away, with any windows it has, without asking about unsaved changes.
    fn remove_tab(&mut self, index: usize) {
        let closed = self.docs.remove(index);
        self.jumps.forget_doc(closed.id);
        self.merge_closed(&closed);
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
            self.message("Hex Mode", crate::i18n::tf!("{what} first: a file this large switches between text and hex by reading it again from disk."));
            return;
        }
        if doc.is_busy() {
            self.message("Please wait", crate::i18n::tr("The file is still loading or saving. This will be available when it finishes.").into());
            return;
        }
        let doc = &mut self.docs[index];
        if doc.hex_mode {
            if !readable_a_block_at_a_time(&doc.text.format) {
                let name = doc.text.format.encoding_name();
                self.message("Hex Mode", crate::i18n::tf!("A file this large can only be shown as text in UTF-8 or a byte set, not {name}. Choose one with Reopen with encoding."));
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
                Err(e) => warn(&crate::i18n::tf!("Couldn't read {}.\n\n{e}", path.display())),
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
                Err(e) => warn(&crate::i18n::tf!("Couldn't read {}.\n\n{e}", path.display())),
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
            self.message("Filter on selection", crate::i18n::tr("Select some text on one line first. Only the lines containing it will show.").into());
            return;
        }
        let needle = doc.text.selection_text();
        if doc.text.is_disk_backed() {
            // Too large to hold: the lines are looked through in the background, and the filter shows when that's done.
            let (store, id) = (doc.text.store().clone(), doc.id);
            if self.big_job.is_some() {
                self.message("Filter on selection", crate::i18n::tr("Please wait for the file being worked through to finish.").into());
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
            self.message("Copy selected", crate::i18n::tr("That's too much to lay out as text. It can be up to 16 MB at a time.").into());
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
            code_only: if hex || !self.find.code_only { None } else { crate::tools::comment_marks(&self.docs[self.active].language_name()) },
        }
    }

    fn run_find(&mut self, backward: bool) {
        if self.no_file() {
            self.find.status = crate::i18n::tr("No file is open.").into();
            return;
        }
        self.record_find(backward);
        note_search(&mut self.find.history, &self.find.find.clone());
        let options = self.search_options(backward);
        if self.docs[self.active].is_busy() {
            self.find.status = crate::i18n::tr("Please wait until the file has finished loading or saving.").into();
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
            self.find.status = crate::i18n::tr("No file is open.").into();
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
                    self.message("Replace All", crate::i18n::tr("That would change more than 1 GB of text, which is too much to hold in memory until it's saved, so nothing has been changed.\n\nReplace in Files on the Find panel can do it, because it writes straight to disk. Save this file first, then close it, and choose it there.").into());
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
            self.find.status = crate::i18n::tr("No file is open.").into();
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
            self.find.status = crate::i18n::tr("Please wait until the file has finished loading or saving.").into();
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
            self.find.status = crate::i18n::tr("Type something to find first.").into();
            return;
        }
        let result = match self.find.scope {
            FileScope::Folder => {
                if self.find.folder.trim().is_empty() || !Path::new(self.find.folder.trim()).is_dir() {
                    self.find.status = crate::i18n::tr("Choose a folder to search first.").into();
                    return;
                }
                find_files::start(PathBuf::from(self.find.folder.trim()), self.find.file_types.clone(), self.find.recursive, &options)
            }
            FileScope::OpenFiles => self.search_open_files(&options),
            FileScope::Project => match &self.project {
                Some(project) if !project.files.is_empty() => find_files::start_list(project.searchable(), &options),
                Some(_) => {
                    self.find.status = crate::i18n::tr("This project has no files yet.").into();
                    return;
                }
                None => {
                    self.find.status = crate::i18n::tr("Open a project first.").into();
                    return;
                }
            },
            FileScope::Favorites => {
                if self.find.favorite_files.is_empty() {
                    self.find.status = crate::i18n::tr("Add a favorite file first.").into();
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
            self.find.status = crate::i18n::tr("Type something to find first.").into();
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
        if crate::clipboards::active() != 0 {
            let text = crate::clipboards::pasted("");
            if !text.is_empty() {
                self.paste_here(&text, ui);
            }
            return;
        }
        let text = arboard::Clipboard::new().and_then(|mut c| c.get_text());
        match text {
            Ok(text) => self.paste_here(&text, ui),
            Err(arboard::Error::ContentNotAvailable) => {}
            Err(e) => warn(&crate::i18n::tf!("Couldn't read the clipboard.\n\n{e}")),
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
        // Words said in full, or made with tf!, are in the language chosen.
        let body = crate::i18n::translation(&body).map_or(body, str::to_string);
        self.dialog = Dialog::Message { title: crate::i18n::tr(title).into(), body };
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
        if let Command::Notebook(act) = command {
            self.notebook_command(act);
            return;
        }
        if let Command::Database(act) = command {
            self.database_command(act, ui.ctx());
            return;
        }
        // In a table, the clipboard commands work on the cell the cursor is in.
        if !self.no_file() && (self.table_command(command, ui) || self.data_command(command, ui)) {
            return;
        }
        // In a comparison, editing goes to the side in use.
        if !self.no_file() && self.compare_command(command, ui) {
            return;
        }
        let hex = self.docs[self.active].hex_mode;
        // Nothing may change a document while it is loading or saving, or while a Replace All runs.
        let changes_document = !matches!(
            command,
            Command::New
                | Command::Open
                | Command::CloseTab
                | Command::CloseAll
                | Command::CloseOthers
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
                | Command::FoldRibbon
                | Command::WhatsNew
                | Command::Guide(_)
                | Command::GitClone
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
                | Command::WindowFont
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
                | Command::CompareFolders
                | Command::CompareFiles
                | Command::ApplyPatch
                | Command::CompareSessions
                | Command::TogglePreview
                | Command::SetPreview(_)
                | Command::ParagraphSetup
                | Command::ProjectOpen
                | Command::ProjectNew
                | Command::ProjectClose
                | Command::ProjectSettings
                | Command::ProjectAddActive
                | Command::NewFromTemplate(_)
                | Command::SaveAsTemplate
                | Command::ProjectAutosave(_)
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
                | Command::Extensions
                | Command::NewAddOn
                | Command::OpenRemote
                | Command::ManageDevices
                | Command::Themes
                | Command::Website
                | Command::SendTo(_)
                | Command::RunOnMachine
                | Command::RunOnBuiltIn
                | Command::RunOnBuiltIn2600
                | Command::RunOnBuiltInVectrex
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
                | Command::Merge
                | Command::GitUseTools
                | Command::GitStopTools
                | Command::ToggleTable
                | Command::OpenInDataViewer
                | Command::QueryRows
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
                | Command::TapeSound
                | Command::SavePreviewAsPdf
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
            self.message("Please wait", crate::i18n::tr("The file is still loading or saving. This will be available when it finishes.").into());
            return;
        }
        if command.edits() && self.docs[self.active].text.read_only {
            self.tell_read_only();
            return;
        }
        if command.needs_whole_file() && self.docs[self.active].narrow.is_some() {
            self.message("Narrowed", crate::i18n::tr("This needs the whole file. Press Show the whole file first.").into());
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
            Command::CloseAll => self.close_all(None),
            Command::CloseOthers => self.close_all(Some(self.active)),
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
            Command::HidePanels => {
                self.panels.hide_all();
                self.python.open = false;
            }
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
            Command::Extensions => self.open_extensions(),
            Command::OpenRemote => self.open_remote_box(None),
            Command::OpenRemoteFolder => self.open_remote_folder_box(),
            Command::OpenZipProject => self.open_zip_project(),
            Command::OpenWebProject => self.open_web_project_box(),
            Command::RemoteProjectFetch => self.remote_project_refresh(false),
            Command::RemoteProjectSync => self.remote_project_refresh(true),
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
            Command::RunOnBuiltIn2600 => self.run_on_target(run_host::Target::BuiltIn2600),
            Command::RunOnBuiltInVectrex => self.run_on_target(run_host::Target::BuiltInVectrex),
            Command::RunOn(device) => self.run_on_target(run_host::Target::Device(device)),
            Command::StopRunning => {
                self.stop_vamos();
                self.stop_all_sessions();
            }
            Command::ChooseAssembler => self.open_assembler_box(),
            Command::Compilers => self.open_compilers_box(),
            Command::AssembleAndSave => self.assemble_and_save(),
            Command::AssembleBinary => self.assemble_binary(),
            Command::NewAs(index) => self.new_of_kind(index),
            Command::NewUsual(name) => self.new_of_usual_kind(name),
            Command::GoToDefinition => self.go_to_definition(),
            Command::FindUses => self.find_uses(),
            Command::RenameLabel => self.open_rename_box(),
            Command::Disassemble => self.open_disassemble_box(),
            Command::SaveDecrunched => self.save_decrunched(),
            Command::NewImage => self.open_new_image_box(),
            Command::OpenUrl => self.open_web_box(),
            Command::Clambers => self.open_clambers(),
            Command::AmigaNdk => self.open_ndk_box(),
            Command::AmigaNewProject => self.open_amiga_project_box(),
            Command::AmigaImportProject => self.import_amiga_project(),
            Command::AmigaMakefiles => self.refresh_amiga_makefiles(true),
            Command::CatalogCheck => self.check_catalogs(),
            Command::CatalogMake => self.make_catalogs(false),
            Command::CatalogFlexCat => self.make_catalogs(true),
            Command::CatalogTranslation => self.new_translation(),
            Command::AutoDocs => self.open_autodocs_at_caret(),
            Command::AmigaHardware => self.open_amiga_hardware(amiga_hw_host::Tab::Registers),
            Command::AmigaTools => self.open_amiga_tools(),
            Command::AmigaIcon => self.open_amiga_icon(ui.ctx()),
            Command::HatariProfile => self.profile_in_hatari(),
            Command::StFloppy => self.make_st_floppy(),
            Command::StTools => self.open_st_tools(),
            Command::C64Tools => self.open_c64_tools(),
            Command::CheckOpticos => self.check_opticos_now(),
            Command::CheckUpdates => self.check_updates_now(),
            Command::GoBackVersion => self.go_back_version(ui.ctx()),
            Command::CleanRecent => panels::clean_recent(&mut self.panels.recent),
            Command::ClearRecent => self.panels.recent.clear(),
            Command::ToggleTerminal => self.toggle_terminal(),
            Command::RunTask(index) => {
                if let Some(task) = self.tasks_here().get(index).cloned() {
                    self.run_task(task);
                }
            }
            Command::DebugTask(index) => {
                if let Some(task) = self.tasks_here().get(index).cloned() {
                    self.debug_task(&task);
                }
            }
            Command::BuildTask => self.run_build_task(),
            Command::TestTask => self.run_test_task(),
            Command::AssemblyTests => self.assembly_tests_here(),
            Command::RasterDesigner => self.open_raster_designer(),
            Command::EditTasks => self.edit_project_tasks(),
            Command::OutdatedPackages => self.outdated_start(),
            Command::SendRequest => self.http_send_request(None),
            Command::SendRequestNumber(number) => self.http_send_request(Some(number)),
            Command::HttpClearCookies => self.http_clear_cookies(),
            Command::BlitterMinterms => self.open_blitter_calculator(),
            Command::TableGenerator => self.open_table_generator(),
            Command::AtariHardware => self.open_atari_hardware(),
            Command::AtariBlitter => self.open_atari_blitter(),
            Command::AtariTimers => self.open_atari_timers(),
            Command::C64RasterLines => self.open_c64_raster(),
            Command::C64RasterIrqs => self.open_c64_irqs(),
            Command::C64Hardware => self.open_c64_hardware(),
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
            Command::AmigaBitplanes => self.open_bitplane_converter(),
            Command::StPictures => self.open_st_converter(),
            Command::C64Pictures => self.open_c64_converter(),
            Command::RetroPictures => self.open_retro_converter(),
            Command::NextPainter => self.open_next_painter(),
            Command::Atari2600Graphics => self.open_atari2600_graphics(),
            Command::CartView => self.open_cart_view(),
            Command::SoundEffects => self.open_sound_effects(),
            Command::Ripper => self.open_ripper(),
            Command::AmigaBootDisk => self.open_boot_disk(),
            Command::DragonDisk => self.open_dragon_disk(),
            Command::Checksums => self.open_binary_tools(binary_tools_host::Tab::Checksums),
            Command::RomPatches => self.open_binary_tools(binary_tools_host::Tab::Patches),
            Command::Entropy => self.open_binary_tools(binary_tools_host::Tab::Entropy),
            Command::RegexTester => self.open_regex_tester(crate::regex_tester::Tab::Pattern),
            Command::SchemaCheck => self.open_regex_tester(crate::regex_tester::Tab::Schema),
            Command::FlashSettings => self.flashing_open = true,
            Command::SerialTerminal => self.open_serial_terminal(),
            Command::EpromProgrammer => self.open_eprom(),
            Command::CodePicture => self.open_code_picture(),
            Command::DocComment => self.write_doc_comment(),
            Command::MarkdownToc => self.markdown_toc(),
            Command::MarkdownTable => self.markdown_table(),
            Command::ServeFolder => self.toggle_live_server(),
            Command::Php(action) => self.php_command(action),
            Command::PyR(action) => self.python_command(action),
            Command::TimeSpent => self.time_spent_open = Some(7),
            Command::HowToCode => self.open_how_to_code(),
            Command::HowToCodeExamples => self.copy_how_to_code_examples(),
            Command::HowToCodeSpectrum => self.open_how_to_code_spectrum(),
            Command::HowToCodeSpectrumExamples => self.copy_how_to_code_spectrum_examples(),
            Command::Formatters => {
                crate::ext_formatters::look_again();
                self.formatters_open = true;
            }
            Command::TileMapEditor => self.open_tile_map(),
            Command::PixelEditor => {
                if self.pixel_editor.is_none() {
                    let mut editor = crate::pixel_editor::Editor::new(crate::pixel_editor::Kind::Characters);
                    // Written the way the file in front reads them: a Spectrum listing sets the UDGs.
                    if !self.no_file() && self.is_spectrum_basic(self.active) {
                        editor.output = crate::pixel_editor::Output::ZxBasicUdgs;
                    } else if let Some(form) = crate::table_gen::Form::for_language(&self.docs[self.active].language_name()) {
                        editor.output = crate::pixel_editor::Output::Table(form);
                    }
                    self.pixel_editor = Some(Box::new(editor));
                }
            }
            Command::EditNewKinds => self.open_new_kinds_box(),
            Command::NewFromTemplate(index) => self.open_template_box(None, index),
            Command::SaveAsTemplate => self.open_save_template_box(),
            Command::ProjectAutosave(choice) => {
                if let Some(project) = &mut self.project {
                    project.autosave = choice;
                }
                self.save_project();
            }
            Command::MachineMemory => self.open_machine_memory(),
            Command::MachineScreen => self.open_machine_screen(),
            Command::MachineBasicOut => self.take_machine_basic(),
            Command::MachineBasicIn => self.put_machine_basic(),
            Command::SourceControl => {
                self.git.open = !self.git.open;
                self.git.refresh();
            }
            Command::GitWizard => self.open_git_wizard(),
            Command::GitClone => self.open_git_clone(),
            Command::FoldRibbon => self.fold_ribbon(),
            Command::ToolbarButtons => self.open_toolbar_box(),
            Command::WhatsNew => self.open_whats_new(),
            Command::Guide(page) => self.open_guide(page),
            Command::Merge => self.merge_files_dialog(),
            Command::ApplyPatch => self.apply_patch_to(None, None),
            Command::CompareSessions => self.open_sessions_box(None),
            Command::ResolveInMerge => self.resolve_in_merge(),
            Command::GitUseTools => self.use_for_git(),
            Command::GitStopTools => self.stop_using_for_git(),
            Command::GitCompareHead => self.compare_with_commit("HEAD"),
            Command::CompareFolders => self.compare_folders_command(),
            Command::CompareFiles => self.compare_two_files(),
            Command::GitHistory => self.git_history(),
            Command::GitStageChange => self.stage_change_at_cursor(),
            Command::GitUndoChange => self.undo_change_at_cursor(),
            Command::ToggleTable => self.toggle_table(),
            Command::OpenInDataViewer => self.open_in_data_viewer(),
            Command::QueryRows => self.open_query_rows(),
            Command::Notebook(_) | Command::Database(_) => {}
            Command::Themes => self.open_theme_box(theme_host::ThemePage::Documents),
            Command::Website => {
                if let Err(problem) = crate::platform::open_web_page("https://limoniter.com") {
                    warn(&problem);
                }
            }
            Command::SendAgain => self.send_again(),
            Command::ManageDevices => self.open_devices_box(),
            Command::OpenFolder => self.choose_folder_to_open(),
            Command::GoToFile => self.open_go_to_file(),
            Command::GoToSymbol(in_project) => self.open_go_to_symbol(in_project),
            Command::FormatDocument => self.lsp_format(self.active, false),
            Command::QuickFix => self.lsp_quick_fix(),
            Command::PeekDefinition => self.peek_definition(),
            Command::StartDebugging => self.start_debugging(),
            Command::DebugPhpListen => self.debug_php_listen(),
            Command::DebugVscodeSetup => self.choose_vscode_launch(),
            Command::DebugStepBack => self.debug_control("stepBack"),
            Command::ColourAs => self.open_language_picker(),
            Command::ProjectOpen => {
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Open project")).add_filter(crate::i18n::tr("Limoniter project"), &[project::EXTENSION]).pick_file() {
                    self.open_project(&path);
                }
            }
            Command::ProjectNew => {
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("New project")).set_file_name(format!("project.{}", project::EXTENSION)).add_filter(crate::i18n::tr("Limoniter project"), &[project::EXTENSION]).save_file() {
                    self.new_project(path);
                }
            }
            Command::ProjectClose => self.close_project(),
            Command::ProjectSettings => self.open_project_settings(),
            Command::ProjectAddActive => {
                if let Some(path) = self.docs[self.active].path.clone() {
                    self.add_to_project([path]);
                } else {
                    self.message("Add to project", crate::i18n::tr("Save this file first, so it has somewhere to be found.").into());
                }
            }
            Command::ProjectAddFiles => {
                if self.project.is_some()
                    && let Some(paths) = with_open_filters(crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Add files to the project"))).pick_files()
                {
                    self.add_to_project(paths);
                }
            }
            Command::ProjectAddFolder => {
                if self.project.is_some()
                    && let Some(folder) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Add a folder to the project")).pick_folder()
                {
                    let (paths, cut_short) = project::files_in_folder(&folder);
                    self.add_to_project(paths);
                    if cut_short {
                        self.message("Add to project", crate::i18n::tr("That folder has a great many files, so only the first 20,000 were added.").into());
                    }
                }
            }
            Command::ProjectBackup => self.backup_project(),
            Command::ProjectRestore => {
                if let Some(project) = &self.project {
                    let backups = project::list_backups(project);
                    if backups.is_empty() {
                        self.message("Restore project", crate::i18n::tr("This project has no backups yet.").into());
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
            Command::SaveSettingsFile => self.save_settings_to_file(),
            Command::RestoreSettingsFile => self.restore_settings_from_file(ui.ctx()),
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
            Command::OpenBeside => self.toggle_beside(None),
            Command::SearchPanel => self.open_search_panel(),
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
            Command::Quit => ui.ctx().send_viewport_cmd(ViewportCommand::Close),
            Command::NewInstance => {
                // From an AppImage, the program is inside it, there only while this copy runs: start the AppImage.
                let exe = std::env::var_os("APPIMAGE").map(PathBuf::from).map_or_else(std::env::current_exe, Ok);
                let started = exe.and_then(|exe| crate::platform::command(exe).spawn());
                if let Err(e) = started {
                    warn(&crate::i18n::tf!("Couldn't open a new window.\n\n{e}"));
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
            Command::WindowFont => self.dialog = Dialog::WindowFont { error: String::new() },
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
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Insert file")).pick_file() {
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
            Command::TapeSound => self.tape_sound(),
            Command::SavePreviewAsPdf => self.save_preview_as_pdf(),
            Command::SaveDocumentAs(target) => self.save_document_as(target),
            Command::Features => self.open_features(),
            Command::SaveSpectrumTape => self.dialog = Dialog::SaveBasic { machine: basic::Machine::Spectrum, run_from: String::new(), error: String::new() },
            Command::SaveNextProgram => self.dialog = Dialog::SaveBasic { machine: basic::Machine::Next, run_from: String::new(), error: String::new() },
            Command::SaveCommodoreProgram => {
                let cbm = basic::CbmMachine::of(self.docs[self.active].text.format.charset);
                self.dialog = Dialog::SaveBasic { machine: basic::Machine::Commodore(cbm), run_from: String::new(), error: String::new() };
            }
            Command::SaveBbcProgram => self.save_tokenised(basic::Machine::Bbc, None),
            Command::SaveBbcZ80Program => self.save_tokenised(basic::Machine::BbcZ80, None),
            Command::SaveAmosProgram => self.save_tokenised(basic::Machine::Amos, None),
            Command::SaveStosProgram => self.save_tokenised(basic::Machine::Stos, None),
            Command::RenameFile => self.begin_rename(),
            Command::SaveLayout => self.dialog = Dialog::SaveLayout { name: String::new(), error: String::new() },
            Command::GatherWindows => {
                // Every box, tool window and add-on panel goes back where it first opens, which is on the screen,
                // and the main window comes up if it was minimised.
                let ctx = ui.ctx();
                ctx.memory_mut(|memory| memory.reset_areas());
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            Command::Surround(open, close) => self.surround_selection(&open.to_string(), &close.to_string()),
            Command::SurroundTag => self.dialog = Dialog::SurroundTag { value: String::new(), error: String::new() },
            Command::WrapAbbreviation => self.dialog = Dialog::WrapAbbreviation { value: String::new(), error: String::new() },
            Command::MoveToBin => self.move_file_to_bin(),
            Command::ShowInExplorer => self.show_in_explorer(),
            Command::OpenWithDefault => self.open_with_usual_program(),
            Command::Snippets => self.dialog = Dialog::Snippets(file_tools::SnippetBox { focus: true, ..Default::default() }),
            Command::RunTool => self.begin_run_tool(),
            Command::StopTool => self.stop_tool(),
            _ if hex => {}
            Command::SplitFile => self.begin_split(),
            Command::PartHistory => self.choose_part(),
            Command::Tag(colour) => self.tag_here(colour),
            Command::UseClipboard(number) => crate::clipboards::choose(number),
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
                    self.message("Hide lines", crate::i18n::tr("Select two or more lines first. The first of them stays showing, and stands for the rest.").into());
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
            Command::Convert(conversion) => self.run_conversion(conversion),
            Command::Upper => self.doc().text.change_case(CaseChange::Upper),
            Command::Lower => self.doc().text.change_case(CaseChange::Lower),
            Command::SentenceCase => self.doc().text.change_case(CaseChange::Sentence),
            Command::TitleCase => self.doc().text.change_case(CaseChange::Title),
            Command::ToggleColumn if self.view.word_wrap => self.dialog = Dialog::ColumnNeedsNoWrap,
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
                    self.message("Word Count", crate::i18n::tr("Please wait for the file being worked through to finish.").into());
                } else {
                    // Counted in the background, a stretch of lines at a time; the result is shown when it's done.
                    let doc = &self.docs[self.active];
                    self.big_job = Some(crate::big_jobs::count(doc.text.store().clone(), doc.text.insert_eol().char_count(), doc.id));
                }
            }
            Command::WordCount if self.docs[self.active].text.selection_too_large_to_copy() => {
                self.message("Word Count", crate::i18n::tr("That selection is too large to count. Select fewer lines.").into());
            }
            Command::WordCount => {
                let doc = &self.docs[self.active];
                let c = doc.text.counts(doc.text.insert_eol().char_count());
                self.message("Word Count", crate::i18n::tf!("Number of Words: {}\nNumber of Unwrapped Lines: {}\nNumber of characters (non-space): {}", c.words, c.lines, c.non_space));
            }
            Command::TrimLeading => self.doc().text.trim_leading(),
            Command::TrimTrailing => self.doc().text.trim_trailing(),
            Command::TabsToSpaces => self.doc().text.tabs_to_spaces(),
            Command::LeadingTabsToSpaces => self.doc().text.leading_tabs_to_spaces(),
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
            Command::ClearBookmarks => self.doc().text.clear_bookmarks(),
            Command::InvertBookmarks => self.doc().text.invert_bookmarks(),
            Command::CopyBookmarked | Command::CutBookmarked => {
                let text = &self.docs[self.active].text;
                ui.ctx().copy_text(text.format.readable_text(&text.bookmarked_text()));
                if command == Command::CutBookmarked {
                    self.doc().text.delete_bookmarked(false);
                }
            }
            Command::DeleteBookmarked | Command::DeleteUnbookmarked => {
                self.doc().text.delete_bookmarked(command == Command::DeleteUnbookmarked);
            }
            Command::FormatJson | Command::CompactJson => self.format_json(command == Command::FormatJson),
            Command::ComputeHash => {
                match self.docs[self.active].text.selection_or_all() {
                    Some(text) => {
                        let scope = if self.docs[self.active].text.has_selection() { "The selection" } else { "This document" };
                        let hash = tools::sha256_text(&text);
                        ui.ctx().copy_text(hash.clone());
                        self.message("Compute Hash", crate::i18n::tf!("{scope}, as text:\n\nSHA-256: {hash}\n\nThe hash has been copied."));
                    }
                    None => self.message("Compute Hash", crate::i18n::tr("This file is too large to hash as text. File properties can hash the saved file, whatever its size.").into()),
                }
            }
            Command::GenerateGuid => self.doc().text.type_text(&tools::new_guid()),
            Command::ConvertNumber => {
                let text = self.docs[self.active].text.selection_for_preview();
                let body = tools::convert_number(&text).unwrap_or_else(|| crate::i18n::tr("Select a whole number first, such as 255, 0xFF or 0b1010.").to_string());
                self.message("Convert Number", body);
            }
            Command::GoToBrace | Command::SelectToBrace => {
                if !self.doc().text.go_to_brace(command == Command::SelectToBrace) {
                    self.message("Braces", crate::i18n::tr("Put the cursor next to a bracket that has a match: ( ) [ ] { }").into());
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
            self.message("Filter on selection", crate::i18n::tr("Editing is off while filtering, because a selection would reach into lines that can't be seen. Press Filter on selection again to end it.").into());
            return;
        }
        self.message("Read-only", crate::i18n::tr("This file is set to read-only. Turn off Read-only on the Edit menu, or click Read-only in the status bar, to change it.").into());
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

    /// Opens `path`, a page of How To Code, in place of the page in front (which is closed unless it has changes
    /// or other windows), carrying the way back and forward with it.
    fn turn_guide_page(&mut self, path: &Path, back: Vec<PathBuf>, forward: Vec<PathBuf>) {
        let old = self.active;
        let view = self.docs[old].preview_view;
        let old_id = self.docs[old].id;
        if !self.open_path(path) {
            return;
        }
        let page = self.active;
        self.docs[page].preview_view = view;
        self.docs[page].guide_back = back;
        self.docs[page].guide_forward = forward;
        if page != old && !self.docs[old].is_modified() && self.docs[old].windows.is_empty() && self.docs[old].id == old_id {
            self.remove_tab(old);
        }
    }

    /// Back or forward to where the cursor was. A place in a document since closed is passed over. On a page of How
    /// To Code, back and forward go through the pages read, as a browser's do.
    fn go_through_history(&mut self, back: bool) {
        let doc = &self.docs[self.active];
        if let Some(here) = doc.path.clone().filter(|path| is_guide_page(path)) {
            let (mut behind, mut ahead) = (doc.guide_back.clone(), doc.guide_forward.clone());
            let next = if back { behind.pop() } else { ahead.pop() };
            if let Some(next) = next {
                if back {
                    ahead.push(here);
                } else {
                    behind.push(here);
                }
                self.turn_guide_page(&next, behind, ahead);
                return;
            }
        }
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
            self.message("JSON", crate::i18n::tr("Turn off Column Mode first, then try again.").into());
            return;
        }
        let Some(text) = doc.text.selection_or_all() else {
            self.message("JSON", crate::i18n::tr("This file is too large to reformat as a whole. Select the part you want instead.").into());
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
            self.message("Sum Selection", crate::i18n::tr("Select a column first: turn on Column Mode (Alt+C), or hold Alt and drag.").into());
            return;
        }
        let s = text.sum_column();
        let sum = format!("{:.*}", s.decimals, s.sum);
        let mut body = crate::i18n::tf!("Sum: {sum}\nNumbers added: {}", s.numbers);
        if s.numbers > 0 {
            body += &format!("\nAverage: {}", s.sum / s.numbers as f64);
        }
        if s.skipped > 0 {
            body += &crate::i18n::tf!("\nSkipped (not numbers): {}", s.skipped);
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
        // Ctrl+0 to Ctrl+9 choose the clipboard.
        for (number, key) in [Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9].into_iter().enumerate() {
            // Not with Alt (macros' keys, and AltGr typing) or Shift (the colour tags').
            let pressed = ctx.input_mut(|i| {
                let at = i.events.iter().position(|e| matches!(e, egui::Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && modifiers.command && !modifiers.alt && !modifiers.shift));
                at.map(|at| i.events.remove(at)).is_some()
            });
            if pressed {
                self.run(Command::UseClipboard(number as u8), ui);
            }
        }
        // Before Ctrl+H, which would take it otherwise.
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl | Modifiers::ALT, Key::H))) && in_editor {
            self.run(Command::PartHistory, ui);
        }
        // In a .http file, Ctrl+Alt+R sends the request the cursor is in.
        if in_editor && !self.no_file() && http_host::is_http_doc(&self.docs[self.active]) && ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl | Modifiers::ALT, Key::R))) {
            self.run(Command::SendRequest, ui);
        }
        // Before Ctrl+S, likewise.
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl | Modifiers::ALT, Key::S))) && in_editor && !self.no_file() {
            self.run(Command::SendAgain, ui);
        }
        // In a Python or R script, Ctrl+Enter sends the line or selection to the console, and Ctrl+Shift+Enter
        // runs the cell the cursor's in, as RStudio and VS Code have them.
        if in_editor && self.code_style(self.active).is_some() {
            if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl | Modifiers::SHIFT, Key::Enter))) {
                self.run(Command::PyR(python_host::PyAction::RunCell), ui);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl, Key::Enter))) {
                self.run(Command::PyR(python_host::PyAction::SendToConsole), ui);
            }
        }
        let always = [
            // Settings, as VS Code and every Mac program have it (Cmd+, there).
            (ctrl, Key::Comma, Command::Settings),
            // Quit, as on a Mac (Cmd+Q) and in most Linux programs.
            (ctrl, Key::Q, Command::Quit),
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
            (Modifiers::ALT, Key::F12, Command::PeekDefinition),
            (Modifiers::CTRL, Key::F5, Command::StartDebugging),
            (Modifiers::SHIFT, Key::F12, Command::FindUses),
            (ctrl, Key::F12, Command::RenameLabel),
            (ctrl, Key::Period, Command::QuickFix),
            (ctrl, Key::Backslash, Command::OpenBeside),
            (Modifiers::SHIFT | Modifiers::ALT, Key::F, Command::FormatDocument),
            (Modifiers::NONE, Key::F6, Command::ToggleTable),
            (Modifiers::NONE, Key::F7, Command::NextMistake),
            (ctrl_shift, Key::R, Command::RecordMacro),
            (ctrl_shift, Key::P, Command::PlayMacro),
            (ctrl_shift, Key::S, Command::SaveAs),
            (ctrl_shift, Key::F, Command::SearchPanel),
            (ctrl_shift, Key::G, Command::SourceControl),
            (ctrl_shift, Key::O, Command::GoToSymbol(false)),
            (ctrl, Key::N, Command::New),
            (ctrl, Key::O, Command::Open),
            (ctrl, Key::P, Command::Print),
            (ctrl, Key::E, Command::GoToFile),
            (ctrl, Key::T, Command::GoToSymbol(true)),
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
            (ctrl, Key::F1, Command::FoldRibbon),
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
            // In an SQL file, Ctrl+Enter runs the statement at the cursor rather than starting a line.
            if self.is_sql_doc(self.active) && ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl, Key::Enter))) {
                self.run(Command::Database(database_host::Act::Run), ui);
            }
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
    /// What the .editorconfig files say of the file in front, read when another comes to the front.
    fn read_editor_config(&mut self) {
        let path = self.docs.get(self.active).and_then(|d| d.path.clone());
        match path {
            Some(path) if self.editor_config.as_ref().map(|(p, _)| p) != Some(&path) => {
                let settings = crate::editorconfig::for_file(&path);
                // A file with nothing in it yet takes its line endings from them.
                if let Some(eol) = settings.end_of_line {
                    let doc = &mut self.docs[self.active];
                    if doc.text.line_count() <= 1 && doc.text.line(0).is_empty() {
                        doc.text.format.preset_eol = eol;
                    }
                }
                self.editor_config = Some((path, settings));
            }
            Some(_) => {}
            None => self.editor_config = None,
        }
    }

    /// The tab's width: what .editorconfig says for the file in front, else the setting.
    /// The view's options as the text's drawn with them: its size kept as chosen, whatever the window's size is.
    fn text_view_options(&self) -> ViewOptions {
        let mut view = self.view;
        view.font_size /= self.window_scale.max(0.1);
        view
    }

    /// Whether the Tab key makes spaces: .editorconfig's word for the file in front, else the project's, else the setting.
    fn spaces_for_tab_now(&self) -> bool {
        self.editor_config.as_ref().and_then(|(_, c)| c.spaces).or_else(|| self.project.as_ref().and_then(|p| p.eff().spaces)).unwrap_or(self.prefs.edit.spaces_for_tab)
    }

    /// The preset new files start with: the project's own choice, else the setting.
    fn new_file_preset_now(&self) -> &'static Preset {
        self.project.as_ref().and_then(|p| p.eff().new_file_preset.as_deref().and_then(codec::preset_by_id)).unwrap_or(self.new_file_preset)
    }

    /// Whether files are formatted as they're saved: the project's word, else the setting.
    pub(super) fn format_on_save_now(&self) -> bool {
        self.project.as_ref().and_then(|p| p.eff().format_on_save).unwrap_or(self.lsp.format_on_save)
    }

    /// The formatter chosen for each language: the project's over the person's own.
    pub(super) fn formatter_choices_now(&self) -> std::collections::HashMap<String, String> {
        let mut choices = self.formatter_choices.clone();
        if let Some(project) = &self.project {
            choices.extend(project.eff().formatters.iter().map(|(language, name)| (language.clone(), name.clone())));
        }
        choices
    }

    fn tab_size_now(&self) -> usize {
        self.editor_config.as_ref().and_then(|(_, c)| c.tab_size()).or_else(|| self.project.as_ref().and_then(|p| p.eff().tab_width)).unwrap_or(self.view.tab_size)
    }

    fn apply_tab_size(&mut self) {
        let size = self.tab_size_now();
        if size == text_buffer::tab_size() {
            return;
        }
        let places: Vec<_> = self.docs.iter().map(|d| (d.text.caret_char_position(), d.text.anchor_char_position())).collect();
        text_buffer::set_tab_size(size);
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
            self.panels.recent.iter().map(|path| format!("recent_file={}\n", path.display())).collect::<String>() + self.recent_in.to_settings(&self.panels.recent).as_str(),
            self.find.history.iter().map(|text| format!("find_history={text}\n")).collect::<String>(),
            self.find.favourites.iter().map(|text| format!("find_favourite={text}\n")).collect::<String>(),
            self.find.favorite_files.iter().map(|path| format!("favorite_file={}\n", path.display())).collect::<String>(),
            self.recent_projects.iter().map(|path| format!("recent_project={}\n", path.display())).collect::<String>(),
            self.library.to_settings() + self.addons.to_settings().as_str() + self.remote_settings().as_str() + self.sends.to_settings().as_str() + self.running.to_settings().as_str() + self.php.to_settings().as_str() + self.git.to_settings().as_str() + self.amiga_palette.to_setting().as_str() + self.control_keys.to_settings().as_str() + self.themes.to_settings().as_str() + crate::vsix::to_settings().as_str(),
            self.current_macro.as_ref().and_then(|current| current.path.as_ref()).map_or_else(String::new, |path| format!("current_macro={}\n", path.display())),
            file_tools::snippets_to_settings(&self.snippets),
            self.assistant.to_settings(),
            self.run_history.iter().map(|line| format!("run_command={line}\n")).collect::<String>() + self.recent_languages.iter().map(|name| format!("recent_language={name}\n")).collect::<String>().as_str() + self.recent_new_kinds.iter().map(|name| format!("recent_new_kind={name}\n")).collect::<String>().as_str() + format!("language_view={}\npalette_width={}\nwindow_scale={}\n", self.language_view.setting(), self.palette_width.round(), self.window_scale).as_str() + self.status_layout.to_settings().as_str() + self.flashing.to_settings().as_str() + self.serial_prefs.to_settings().as_str() + self.eprom_prefs.to_settings().as_str() + crate::ext_formatters::to_settings(&self.formatter_choices).as_str() + WINDOW_FONT.lock().ok().and_then(|f| f.clone()).map(|f| format!("window_font={}\n", f.display())).unwrap_or_default().as_str() + new_kinds::kinds_to_settings(&self.new_kinds).as_str() + self.lsp.to_settings().as_str() + self.compare_rules.to_settings().as_str() + crate::layouts::to_settings(&self.layouts).as_str() + crate::dap::kept_to_settings(&self.debug_configs).as_str() + self.database.to_settings().as_str() + self.python.to_settings().as_str() + self.http_ui.to_settings().as_str() + self.side_tabs.to_settings().as_str() + self.toolbar.to_settings().as_str()
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

    /// What a layout keeps: which panels are open, and the output, assistant, map, file tabs and status bar.
    fn layout_now(&self) -> String {
        format!(
            "{}output_window={}\nassistant_panel={}\ndocument_map={}\nfile_tabs={}\nstatus_bar={}\n",
            self.panels.open_ones(),
            self.show_results,
            self.assist_panel.open,
            self.view.document_map,
            self.show_file_tabs,
            self.show_status
        )
    }

    /// Puts a saved layout back. Anything it doesn't mention stays as it is.
    fn apply_layout(&mut self, index: usize) {
        let Some(layout) = self.layouts.get(index).cloned() else {
            return;
        };
        self.panels.read_settings(&layout.open);
        let switch = |name: &str| crate::layouts::switch(&layout.open, name);
        self.show_results = switch("output_window").unwrap_or(self.show_results);
        self.assist_panel.open = switch("assistant_panel").unwrap_or(self.assist_panel.open);
        self.view.document_map = switch("document_map").unwrap_or(self.view.document_map);
        self.show_file_tabs = switch("file_tabs").unwrap_or(self.show_file_tabs);
        self.show_status = switch("status_bar").unwrap_or(self.show_status);
        // The one used last comes first.
        let used = self.layouts.remove(index);
        self.layouts.insert(0, used);
    }

    /// The encodings chosen for particular files, as their file holds them.
    fn encodings_text(&self) -> String {
        encoding_prefs_path().and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default()
    }

    /// Tools, Save settings to a file.
    fn save_settings_to_file(&mut self) {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save settings to a file")).set_file_name(crate::i18n::tf!("Limoniter settings {today}.txt")).add_filter(crate::i18n::tr("Text"), &["txt"]).save_file() else {
            return;
        };
        let text = crate::settings_file::bundle(&self.current_settings(), &self.encodings_text(), env!("CARGO_PKG_VERSION"), &today);
        match std::fs::write(&path, text) {
            Ok(()) => self.message("Save settings", crate::i18n::tf!("Saved to {}.\n\nPasswords for servers aren't in it: they're kept in this computer's keychain.", path.display())),
            Err(e) => self.message("Save settings", crate::i18n::tf!("Couldn't save {}: {e}", path.display())),
        }
    }

    /// Tools, Restore settings from a file: they're put in place of these, at once, and the settings as they were
    /// are kept beside the settings file in case they're wanted back.
    fn restore_settings_from_file(&mut self, ctx: &Context) {
        let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Restore settings from a file")).add_filter(crate::i18n::tr("Text"), &["txt"]).pick_file() else {
            return;
        };
        let read = std::fs::read_to_string(&path).map_err(|e| crate::i18n::tf!("Couldn't read {}: {e}", path.display()));
        let (settings, encodings) = match read.and_then(|text| crate::settings_file::unbundle(&text)) {
            Ok(both) => both,
            Err(problem) => return self.message("Restore settings", problem),
        };
        if !ask_yes_no("Restore settings", "Put the settings in this file in place of the ones Limoniter has now?\n\nThe ones it has now are kept as a file beside them, in case you want them back.") {
            return;
        }
        let kept = self.write_settings_backup();
        self.apply_settings(ctx, settings);
        if let Some(prefs_path) = encoding_prefs_path() {
            let _ = std::fs::write(&prefs_path, &encodings);
            self.encoding_prefs = EncodingPrefs::load(Some(prefs_path));
        }
        self.save_settings_if_changed();
        let note = kept.map_or_else(String::new, |kept| crate::i18n::tf!("\n\nThe settings it had before are in {}.", kept.display()));
        self.message("Restore settings", crate::i18n::tf!("The settings from {} are in use.{note}", path.display()));
    }

    /// A copy of the settings as they are now, beside the settings file.
    fn write_settings_backup(&self) -> Option<PathBuf> {
        let folder = settings_folder()?;
        let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
        let backup = folder.join(format!("settings before restore {stamp}.txt"));
        let text = crate::settings_file::bundle(&self.current_settings(), &self.encodings_text(), env!("CARGO_PKG_VERSION"), &stamp.to_string());
        std::fs::create_dir_all(&folder).ok()?;
        std::fs::write(&backup, text).ok().map(|()| backup)
    }

    /// Takes on settings read from a file, as if Limoniter had started with them: everything the settings file
    /// holds, read by the same code that reads it at start.
    fn apply_settings(&mut self, ctx: &Context, settings: String) {
        let font_file = list_setting(&settings, "font_file").pop().map(PathBuf::from).filter(|path| path.exists());
        if font_file != self.font_file {
            install_fonts(ctx, font_file.as_deref());
        }
        let fresh = Self::build(settings, font_file, None);
        self.view = fresh.view;
        self.prefs = fresh.prefs;
        // The panels keep what they're showing; only which are open, and the tags, come from the file.
        self.panels.custom_tags.clear();
        self.panels.read_settings(&fresh.saved_settings);
        self.panels.recent = fresh.panels.recent;
        self.recent_in = fresh.recent_in;
        self.show_file_tabs = fresh.show_file_tabs;
        self.side_tabs = fresh.side_tabs;
        self.toolbar = fresh.toolbar;
        self.show_status = fresh.show_status;
        self.new_file_preset = fresh.new_file_preset;
        self.recent_projects = fresh.recent_projects;
        self.find.history = fresh.find.history;
        self.find.favourites = fresh.find.favourites;
        self.find.favorite_files = fresh.find.favorite_files;
        self.font_file = fresh.font_file;
        self.library = fresh.library;
        self.addons = fresh.addons;
        self.remote_sites = fresh.remote_sites;
        self.known_hosts = fresh.known_hosts;
        self.remote_recent = fresh.remote_recent;
        self.sends = fresh.sends;
        self.running = fresh.running;
        self.git = fresh.git;
        self.amiga_palette = fresh.amiga_palette;
        crate::ansi_art::set_amiga_palette(self.amiga_palette);
        self.control_keys = fresh.control_keys;
        self.themes = fresh.themes;
        self.applied_window = None;
        self.snippets = fresh.snippets;
        self.layouts = fresh.layouts;
        self.run_history = fresh.run_history;
        self.assistant = fresh.assistant;
        self.new_kinds = fresh.new_kinds;
        self.compare_rules = fresh.compare_rules;
        self.database.kept = fresh.database.kept;
        self.database.for_file = fresh.database.for_file;
        // Servers already running carry on; only whether they're used, and which, change.
        self.lsp.on = fresh.lsp.on;
        self.lsp.settings = fresh.lsp.settings;
        self.current_macro = fresh.current_macro;
        text_buffer::set_tab_size(self.view.tab_size);
        text_buffer::set_source_view(self.view.source_view);
        ctx.request_repaint();
    }

    fn side_panels(&mut self, ui: &mut Ui) {
        let tabs: Vec<TabInfo> = self
            .docs
            .iter()
            .filter(|doc| !doc.placeholder)
            .map(|doc| TabInfo { title: if doc.is_modified() { format!("{} *", doc.name()) } else { doc.name() }, path: doc.path.clone() })
            .collect();
        let active = self.active;
        let mut actions = Vec::new();
        let mut project_changed = false;
        let mut compare_wanted = Vec::new();
        panels::reveal_sides(ui.ctx(), self.view.panels_hide);
        self.search_panel(ui);
        let doc = &self.docs[active];
        let view = DocView { id: doc.id, text: &doc.text, hex: &doc.hex, format: &doc.text.format, path: doc.path.as_deref(), hex_mode: doc.hex_mode, name: doc.name(), language: doc.language_name(), no_file: doc.placeholder, syntax: doc.resolved_syntax.flatten(), image: doc.browser.as_ref().map(|b| b.view()) };
        self.panels.show(ui, &view, &tabs, active, &mut actions);
        let code_acts = self.panels.show_code(ui, &view);
        let library_actions = self.panels.show_library(ui, &mut self.library);
        let history_rows = if self.panels.history && !doc.placeholder { doc.text.history_rows() } else { Vec::new() };
        let history_actions = self.panels.show_history(ui, &history_rows, doc.hex_mode);
        self.variables_panel(ui);
        self.console_panel(ui);
        let entries = self.git.status.as_ref().map(|s| s.entries.as_slice()).unwrap_or_default();
        self.panels.project_panel.explorer.set_git(self.git.repo.as_deref(), entries);
        let doc = &self.docs[active];
        let project_actions = self.panels.show_project(ui, self.project.as_ref(), doc.path.as_deref());
        let todo_view = DocView { id: doc.id, text: &doc.text, hex: &doc.hex, format: &doc.text.format, path: doc.path.as_deref(), hex_mode: doc.hex_mode, name: doc.name(), language: doc.language_name(), no_file: doc.placeholder, syntax: doc.resolved_syntax.flatten(), image: None };
        let todo_acts = self.panels.show_todos(ui, &todo_view, self.project.as_ref());
        self.database_panel(ui);
        self.containers_panel(ui);
        self.http_panel(ui);
        let mut explorer_acts = Vec::new();
        for action in project_actions {
            match action {
                project::PanelAction::Explorer(act) => explorer_acts.push(act),
                project::PanelAction::Open(path) => actions.push(Action::OpenPath(path)),
                project::PanelAction::Remove(path) => {
                    if let Some(project) = &mut self.project {
                        project.remove(&path);
                    }
                    project_changed = true;
                }
                other => compare_wanted.push(other),
            }
        }
        if project_changed {
            self.save_project();
        }
        for action in compare_wanted {
            self.project_compare(action);
        }
        for act in todo_acts {
            match act {
                crate::todo_panel::TodoAct::GoToLine(line) => actions.push(Action::GoToLine(line)),
                crate::todo_panel::TodoAct::Open(path, line) => self.go_to_place(&path.to_string_lossy(), line + 1),
            }
        }
        for act in explorer_acts {
            // An Amiga project's makefiles follow the files made, renamed and deleted in the folder.
            let touched: Vec<PathBuf> = match &act {
                crate::explorer::Act::Create { path, .. } | crate::explorer::Act::Delete(path) => vec![path.clone()],
                crate::explorer::Act::Rename { from, to } => vec![from.clone(), to.clone()],
                _ => Vec::new(),
            };
            self.explorer_act(act, ui);
            for path in touched {
                self.amiga_files_changed(&path);
            }
        }
        for act in code_acts {
            match act {
                crate::code_panels::CodeAct::GoTo(None, line) => actions.push(Action::GoToLine(line)),
                crate::code_panels::CodeAct::GoTo(Some(path), line) => self.go_to_place(&path.to_string_lossy(), line + 1),
                crate::code_panels::CodeAct::AskServer => self.lsp_call_hierarchy(),
            }
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
        panels::mark_tucked(ui.ctx());
    }

    /// Shows this file as it will look, beside the text, keeping up as it's edited. Returns true if it's to fill the
    /// text area instead. It only does that with something to show: otherwise the text stays, with the reason beside it.
    fn preview_panel(&mut self, ui: &mut Ui) -> bool {
        let doc = &self.docs[self.active];
        let view = doc.preview_view;
        let kind = Self::preview_kind(doc);
        let mut note = None;
        match kind {
            _ if doc.placeholder => note = Some(crate::i18n::tr("No file open").to_string()),
            None => note = Some(crate::i18n::tr("There's no preview for this kind of file yet. There are previews for Markdown, web pages, SVG drawings, PC ANSI art, PETSCII, Commodore screens, teletext, ZX Spectrum text, tokenised BASIC, tracker music modules (MOD, S3M, XM and IT), sound files (MP3, WAV, FLAC, Ogg, AIFF, 8SVX, VOC and AU), and pictures from the Amiga, Atari ST, Commodore 64, Mac, RISC OS, PC (PCX, GEM IMG), ZX Spectrum and Kodak Photo CD.").to_string()),
            Some(kind) if kind.reads_bytes() && !doc.text.is_disk_backed() => {
                // These are made from the exact bytes, so they work from hex mode as well as text.
                let version = if doc.hex_mode { doc.hex.version() | 1 << 63 } else { doc.text.version() };
                let key = (doc.id, version, kind);
                if self.preview_of != Some(key) {
                    // Only as much as could be shown is read, so a huge file isn't read in just to say it's too large.
                    let music = matches!(kind, preview::Kind::Module | preview::Kind::Sound);
                    let enough = if music { preview::MOST_SOUND_BYTES } else { crate::pictures::MOST_PICTURE_BYTES.max(preview::MOST_PREVIEW_BYTES) } + 1;
                    let bytes = if doc.hex_mode { doc.hex.read(0, enough) } else { doc.text.encode(&doc.text.format, Lossy::Omit).unwrap_or_default() };
                    self.preview_screen = None;
                    self.preview_picture = None;
                    self.preview_cycling = None;
                    self.preview_blocks.clear();
                    self.preview_note = None;
                    // Pictures are decoded rather than laid out, so they can be larger: a Photo CD image is 4 to 6 MB.
                    let most = match kind {
                        preview::Kind::Picture => crate::pictures::MOST_PICTURE_BYTES,
                        preview::Kind::Module | preview::Kind::Sound => preview::MOST_SOUND_BYTES,
                        _ => preview::MOST_PREVIEW_BYTES,
                    };
                    if bytes.len() > most {
                        note = Some(crate::i18n::tf!("This file is too large to preview. It can be up to {} MB.", most >> 20));
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
            Some(_) if doc.hex_mode => note = Some(crate::i18n::tr("Switch to text mode to preview this file.").to_string()),
            Some(_) if doc.text.is_disk_backed() => note = Some(crate::i18n::tr("This file is too large to preview. It is being kept on disk.").to_string()),
            Some(kind) => {
                let key = (doc.id, doc.text.version(), kind);
                if self.preview_of != Some(key) {
                    let lines = doc.text.all_lines();
                    let size: usize = lines.iter().map(|line| line.len() + 1).sum();
                    if size > preview::MOST_PREVIEW_BYTES {
                        self.preview_blocks.clear();
                        note = Some(crate::i18n::tr("This file is too large to preview. It can be up to 4 MB.").to_string());
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
                    if crate::access::called(ui.add(Button::new(RichText::new(crate::i18n::tr("\u{D7}")).color(theme::text_dim())).frame(false)), "Close").on_hover_text(crate::i18n::tr("Close")).clicked() {
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
        if matches!(kind, Some(preview::Kind::Picture | preview::Kind::Svg)) {
            // A Workbench icon's details can be changed, in a window of their own.
            if self.docs[self.active].picture == Some(crate::pictures::Format::AmigaIcon) && ui.button(crate::i18n::tr("Change the icon's tool types\u{2026}")).on_hover_text(crate::i18n::tr("Its kind, default tool, stack and tool types (Amiga, Workbench icon)")).clicked() {
                self.pending_command = Some(Command::AmigaIcon);
            }
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
        if matches!(kind, Some(preview::Kind::Module | preview::Kind::Sound)) && self.preview_note.is_none() {
            let doc = &self.docs[self.active];
            let head = if doc.hex_mode { doc.hex.read(0, 1084) } else { Vec::new() };
            if kind == Some(preview::Kind::Sound) || music_host::playable(&head, &doc.name()) {
                self.music_bar(ui);
            }
        }
        // A page of How To Code has a bar of its own, as a browser has: back, forward, the contents and close.
        if let Some(page) = self.docs[self.active].path.clone().filter(|path| is_guide_page(path)) {
            let (can_back, can_forward) = (!self.docs[self.active].guide_back.is_empty(), !self.docs[self.active].guide_forward.is_empty());
            let mut picked = None;
            ui.horizontal(|ui| {
                if ui.add_enabled(can_back, Button::new(crate::i18n::tr("\u{2039} Back"))).on_hover_text(crate::i18n::tr("The page before (Alt+Left)")).clicked() {
                    picked = Some(0);
                }
                if ui.add_enabled(can_forward, Button::new(crate::i18n::tr("Forward \u{203A}"))).on_hover_text(crate::i18n::tr("The page after (Alt+Right)")).clicked() {
                    picked = Some(1);
                }
                let contents = page.with_file_name("README.md");
                if ui.add_enabled(page != contents, Button::new(crate::i18n::tr("Contents"))).clicked() {
                    picked = Some(2);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::i18n::tr("Close")).on_hover_text(crate::i18n::tr("Close the guide (Ctrl+W)")).clicked() {
                        picked = Some(3);
                    }
                });
            });
            ui.separator();
            match picked {
                Some(0) => return self.go_through_history(true),
                Some(1) => return self.go_through_history(false),
                Some(2) => {
                    let mut back = std::mem::take(&mut self.docs[self.active].guide_back);
                    back.push(page.clone());
                    return self.turn_guide_page(&page.with_file_name("README.md"), back, Vec::new());
                }
                Some(_) => return self.close_tab(self.active),
                None => {}
            }
        }
        let folder = self.docs[self.active].path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        match (&self.preview_screen, kind) {
            (Some(screen), Some(kind)) if kind.is_screen() => crate::screen::draw(ui, screen),
            _ => preview::draw_in(ui, &self.preview_blocks, folder.as_deref()),
        }
        // A link to a file beside this one: opened here, in the same view.
        if let Some(link) = ui.ctx().data_mut(|d| d.remove_temp::<String>(egui::Id::unique(preview::OPEN_LOCAL)))
            && let Some(folder) = folder
        {
            let view = self.docs[self.active].preview_view;
            let path = folder.join(link);
            // A link in How To Code turns the page in the same tab, as a browser does, keeping the way back.
            let from = self.docs[self.active].path.clone().filter(|from| is_guide_page(from) && is_guide_page(&path));
            if let Some(from) = from {
                let mut back = std::mem::take(&mut self.docs[self.active].guide_back);
                back.push(from);
                self.turn_guide_page(&path, back, Vec::new());
            } else if self.open_path(&path) && self.docs[self.active].path.as_deref().is_some_and(|p| same_path(p, &path)) {
                self.docs[self.active].preview_view = view;
            }
        }
    }

    /// The preview with the whole text area to itself, and the way back to the text.
    /// Shows a tab (or the one used before this) beside the one in front; or, shown already, stops. With one tab,
    /// it's split in two instead.
    pub(super) fn toggle_beside(&mut self, other: Option<u64>) {
        if self.beside.is_some() && other.is_none() {
            self.beside = None;
            return;
        }
        let front = self.docs[self.active].id;
        let other = other.filter(|id| *id != front).or_else(|| self.used_order.iter().copied().find(|id| *id != front && self.docs.iter().any(|d| d.id == *id)));
        match other {
            Some(other) => self.beside = Some(Beside { other, front, right: true, share: 0.5 }),
            None => self.toggle_split(true),
        }
    }

    /// Shows a tab beside the one in front: on the right, or the left.
    pub(super) fn show_beside(&mut self, other: u64, right: bool) {
        let front = self.docs[self.active].id;
        // Not the one in front beside itself.
        if other == front {
            return;
        }
        self.beside = Some(Beside { other, front, right, share: self.beside.as_ref().map_or(0.5, |b| b.share) });
    }

    /// Where the tab in front and the one beside it go, and which that is; None with nothing beside. A press in
    /// the one beside brings it to the front, where it is.
    fn beside_layout(&mut self, ui: &mut Ui, input_on: bool) -> Option<(egui::Rect, egui::Rect, usize)> {
        const GAP: f32 = 5.0;
        let front = self.docs[self.active].id;
        let beside = self.beside.as_mut()?;
        // A tab chosen above: the one beside, swapped over; any other, in front's place.
        if front != beside.front {
            if front == beside.other {
                beside.other = beside.front;
                beside.right = !beside.right;
            }
            beside.front = front;
        }
        let Some(other) = self.docs.iter().position(|d| d.id == beside.other) else {
            self.beside = None;
            return None;
        };
        let whole = ui.available_rect_before_wrap();
        let x = whole.min.x + ((whole.width() - GAP) * beside.share).round();
        let left = egui::Rect::from_min_max(whole.min, egui::pos2(x, whole.max.y));
        let gap = egui::Rect::from_min_max(egui::pos2(x, whole.min.y), egui::pos2(x + GAP, whole.max.y));
        let right = egui::Rect::from_min_max(egui::pos2(x + GAP, whole.min.y), whole.max);
        let handle = ui.interact(gap, egui::Id::unique("beside-divider"), egui::Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        if handle.dragged()
            && let Some(pointer) = handle.interact_pointer_pos()
        {
            beside.share = ((pointer.x - whole.min.x) / whole.width().max(1.0)).clamp(0.15, 0.85);
        }
        ui.painter().rect_filled(gap, 0.0, theme::border());
        let (mine, theirs) = if beside.right { (left, right) } else { (right, left) };
        // A press in the other one, or (with focus following the mouse) the pointer moving into it with no button held.
        let follow = self.view.focus_follows_mouse && ui.input(|i| !i.pointer.any_down() && i.pointer.delta() != egui::Vec2::ZERO);
        if input_on && ui.input(|i| (i.pointer.primary_pressed() || follow) && i.pointer.hover_pos().is_some_and(|p| theirs.contains(p))) {
            // Swapped before either's drawn, so the press lands in the one now in front.
            let was = self.active;
            self.active = other;
            beside.other = self.docs[was].id;
            beside.front = self.docs[other].id;
            beside.right = !beside.right;
            ui.memory_mut(|m| m.request_focus(views::pane_id(0)));
            return Some((theirs, mine, was));
        }
        Some((mine, theirs, other))
    }

    /// The tab beside the one in front: its text, as it is, to read and scroll; anything else, by its name.
    fn draw_beside(&mut self, ui: &mut Ui, index: usize, view: ViewOptions) {
        // In its own colours, which are put back for the one in front after.
        theme::set_document(self.document_theme_of(index));
        self.draw_beside_in_its_colours(ui, index, view);
        theme::set_document(self.document_theme_of(self.active));
    }

    fn draw_beside_in_its_colours(&mut self, ui: &mut Ui, index: usize, view: ViewOptions) {
        let shown_as_notebook = self.notebook_shown(index);
        let doc = &mut self.docs[index];
        let plain = !doc.hex_mode && !doc.placeholder && doc.browser.is_none() && doc.game.is_none() && doc.compare.is_none() && doc.merge.is_none() && doc.table.is_none() && doc.data.is_none() && !shown_as_notebook;
        if plain {
            doc.update_syntax();
            views::text_pane(ui, &mut doc.text, &mut doc.text_state, &mut self.clip, view, false, Pane { number: 2, active: false, ..Pane::default() });
        } else {
            let name = doc.name();
            ui.centered_and_justified(|ui| ui.label(RichText::new(crate::i18n::tf!("{name}: click to bring it to the front")).color(theme::text_faint())));
        }
    }

    /// The document in front, in the middle of the window; and, beside it, another tab's if one's shown there.
    fn central(&mut self, ui: &mut Ui, full_preview: bool, view: ViewOptions, input_on: bool) -> Option<ContextCommand> {
        // With every tab closed: the start page, unless it's been turned off for an empty window.
        if self.no_file() && self.view.start_page {
            self.start_page(ui);
            return None;
        }
        let Some((mine, theirs, other)) = self.beside_layout(ui, input_on) else {
            return self.draw_front(ui, full_preview, view, input_on);
        };
        ui.scope_builder(egui::UiBuilder::new().max_rect(theirs), |ui| self.draw_beside(ui, other, view));
        let command = ui.scope_builder(egui::UiBuilder::new().max_rect(mine), |ui| self.draw_front(ui, full_preview, view, input_on)).inner;
        // A line along the top says which is in front.
        ui.painter().rect_filled(egui::Rect::from_min_size(mine.min, egui::vec2(mine.width(), 2.0)), 0.0, theme::accent());
        command
    }

    /// The document in front, as it's shown: text, hex, a table, a comparison and the rest.
    fn draw_front(&mut self, ui: &mut Ui, full_preview: bool, view: ViewOptions, input_on: bool) -> Option<ContextCommand> {
        let index = self.active;
        // A command from this frame may have changed what's in front since the preview was prepared.
        if full_preview && !self.docs[index].placeholder && self.docs[index].preview_view == settings::PreviewView::Full {
            self.full_preview(ui);
            return None;
        }
        if self.docs[index].browser.is_some() {
            self.browser_view(ui);
            return None;
        }
        if self.docs[index].data.is_some() {
            self.data_view(ui, input_on);
            return None;
        }
        if self.docs[index].game.is_some() {
            self.clambers_view(ui);
            return None;
        }
        if self.docs[index].compare.is_some() {
            self.compare_view(ui, input_on);
            return None;
        }
        if self.docs[index].merge.is_some() && !self.docs[index].hex_mode {
            return self.merge_view(ui, view, input_on);
        }
        if self.notebook_shown(index) {
            self.notebook_view(ui, input_on);
            return None;
        }
        // Where the file is and what the cursor's in, above the text; and what's kept at the top as it scrolls.
        let plain_text = !self.docs[index].placeholder && !self.docs[index].hex_mode && self.docs[index].table.is_none();
        if plain_text && self.view.breadcrumbs {
            self.breadcrumbs(ui, index);
        }
        let sticky = if plain_text { self.sticky_line(index) } else { None };
        self.docs[index].text_state.sticky = sticky;
        let doc = &mut self.docs[index];
        if doc.placeholder {
            return None;
        }
        doc.update_syntax();
        doc.refresh_fold_view();
        let folded = doc.filter.is_none() && doc.fold_view_of.is_some();
        let total = doc.text.line_count();
        let filter = doc.filter.as_ref().map(|filter| views::Shown::Lines(filter.lines.as_slice())).or(folded.then_some(views::Shown::AllBut { hidden: doc.fold_view.as_slice(), total }));
        if doc.table.is_some() && !doc.hex_mode {
            self.table_view(ui, input_on);
            return None;
        }
        if doc.hex_mode {
            let fields = if self.panels.structure { self.panels.structure_panel.marks(doc.id, doc.hex.version()) } else { None };
            views::hex_view(ui, &mut doc.hex, &mut doc.hex_state, &doc.text.format, view, input_on, fields)
        } else if let Some(split) = &mut doc.split {
            draw_split(ui, &mut doc.text, &mut doc.text_state, split, &mut self.clip, filter, folded, view, input_on)
        } else {
            views::text_pane(ui, &mut doc.text, &mut doc.text_state, &mut self.clip, view, input_on, Pane { filter, folded, ..Pane::default() })
        }
    }

    fn full_preview(&mut self, ui: &mut Ui) {
        self.doc().update_syntax();
        let mut chosen = None;
        egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 4)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Preview, full screen")).color(theme::text_dim()).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::i18n::tr("Text only")).on_hover_text(crate::i18n::tr("Back to the text, to edit it")).clicked() {
                        chosen = Some(settings::PreviewView::Text);
                    }
                    if ui.button(crate::i18n::tr("Split screen")).on_hover_text(crate::i18n::tr("The text and the preview side by side")).clicked() {
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
            Action::StructureHelp => self.open_page(crate::i18n::tr("Structure templates"), crate::structure_panel::help()),
            Action::GoToLine(line) => {
                let doc = self.doc();
                if !doc.hex_mode {
                    doc.text.go_to(&(line + 1).to_string());
                }
            }
            Action::InsertEntry { .. } | Action::InsertCharacter(_) | Action::InsertBytes(_) | Action::AskInsertByte | Action::Paste(_) | Action::WrapTag(..) if self.docs[self.active].text.read_only => self.tell_read_only(),
            Action::InsertEntry { .. } | Action::InsertCharacter(_) | Action::InsertBytes(_) | Action::AskInsertByte | Action::Paste(_) | Action::WrapTag(..) if self.docs[self.active].is_busy() || self.pending.is_some() => {
                self.message("Please wait", crate::i18n::tr("The file is still loading or saving. This will be available when it finishes.").into());
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
            self.message("Please wait", crate::i18n::tr("The file is still loading or saving. This will be available when it finishes.").into());
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
            let answer = crate::sys_dialogs::message_dialog()
                .set_title(Purpose::Reopen.title())
                .set_level(MessageLevel::Warning)
                .set_description(crate::i18n::tf!("{} has unsaved changes.\n\nSave them first? Choosing No discards them.", self.docs[index].name()))
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
                self.message(Purpose::Reopen.title(), crate::i18n::tr("This document hasn't been saved, so there are no bytes to read again. Convert to encoding keeps the text and changes how it's stored.").into());
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
            Err(problem) => warn(&crate::i18n::tf!("Couldn't open {}.\n\n{problem}", path.display())),
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
                return self.message(CONVERT_TITLE, crate::i18n::tf!("A file this large can only be converted in place to UTF-8 or a one-byte character set, since it's read from disk a part at a time.\n\nFile, Save a copy can write it in {name}: the copy is written straight to disk."));
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
                self.message("Save a copy", crate::i18n::tr("Please wait for the file being worked through to finish.").into());
                return;
            }
            let doc = &self.docs[index];
            if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save a copy")).set_file_name(doc.name()).save_file() {
                self.big_job = Some(crate::big_jobs::save_converted(doc.text.store().clone(), doc.text.format.clone(), target, lossy, path, doc.id));
            }
            return;
        }
        if self.docs[index].hex.is_on_disk() {
            self.message("Save a copy", crate::i18n::tr("This file is too large to copy into another encoding, because that would have to read it all into memory. Save As can copy it in the encoding it has.").into());
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
        if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save a copy")).set_file_name(name).save_file()
            && let Err(problem) = codec::write_file(&path, &bytes)
        {
            warn(&crate::i18n::tf!("Couldn't save {}.\n\n{problem}", path.display()));
        }
    }

    fn run_menu_action(&mut self, action: MenuAction) {
        let index = self.active;
        if self.docs[index].narrow.is_some() && !matches!(action, MenuAction::Forget | MenuAction::ToggleSourceView | MenuAction::AskInsertByte | MenuAction::ChoosePreset) {
            self.message("Narrowed", crate::i18n::tr("This changes how the whole file is read or saved. Press Show the whole file first.").into());
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
                crate::fit::Dialog::new(crate::i18n::tr("Insert byte")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(crate::i18n::tr("Byte value in hexadecimal, such as 9B or 1B 5B:"));
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
                                *error = crate::i18n::tr("That isn't a whole number of bytes in hexadecimal.").to_string();
                            }
                        }
                    }
                }
            }
            Dialog::WrapColumn { value, error, focus } => {
                let mut go = false;
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                crate::fit::Dialog::new(crate::i18n::tr("Wrap to line ends")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(crate::i18n::tr("Break long lines at a space, so they end by this column:"));
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
                        None => *error = crate::i18n::tf!("That has to be a whole number from {} to {}.", reformat::LEAST_WIDTH, reformat::MOST_WIDTH),
                    }
                }
            }
            Dialog::ParagraphSetup { width, left, first, align, error } => {
                let mut go = false;
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                crate::fit::Dialog::new(crate::i18n::tr("Paragraph setup")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    egui::Grid::new("paragraph-grid").num_columns(2).show(ui, |ui| {
                        for (label, field) in [("Lines end by column:", &mut *width), ("Left margin:", &mut *left), ("First line indent:", &mut *first)] {
                            ui.label(crate::i18n::tr(label));
                            ui.text_edit_singleline(field);
                            ui.end_row();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr("Alignment:"));
                        for (name, value) in [("Left", Align::Left), ("Center", Align::Center), ("Right", Align::Right), ("Justify", Align::Justify)] {
                            ui.radio_value(align, value, crate::i18n::tr(name));
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
                        _ => *error = crate::i18n::tf!("Those have to be whole numbers, and lines have to end by column {} or more.", reformat::LEAST_WIDTH),
                    }
                }
            }
            Dialog::ProjectSettings(draft) => {
                let base = self.project.as_ref().map(|p| p.folder()).unwrap_or_default();
                match project_settings_host::window(ctx, draft, &base) {
                    project_settings_host::Result::Open => {}
                    project_settings_host::Result::Cancel => self.close_dialog(),
                    project_settings_host::Result::Ok => {
                        if let Dialog::ProjectSettings(draft) = std::mem::replace(&mut self.dialog, Dialog::None) {
                            self.apply_project_settings(*draft);
                        }
                    }
                }
            }
            Dialog::RestorePick { backups, chosen } => {
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                let mut go = false;
                crate::fit::Dialog::new(crate::i18n::tr("Restore project")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(crate::i18n::tr("Put the files back from this backup:"));
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        for (place, backup) in backups.iter().enumerate() {
                            ui.radio_value(chosen, place, crate::i18n::tf!("{} ({} files)", backup.name, backup.files.len()));
                        }
                    });
                    ui.add_space(4.0);
                    ui.label(crate::i18n::tr("This replaces the files on disk. A backup of how they are now is taken first."));
                    ui.label(crate::i18n::tr("Files open here with unsaved changes are left alone."));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        go = ui.button(crate::i18n::tr("Restore")).clicked();
                        cancelled |= ui.button(crate::i18n::tr("Cancel")).clicked();
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
            Dialog::ComparePick(others) => {
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                let (mut open_file, mut from_disk) = (None, false);
                crate::fit::Dialog::new(crate::i18n::tr("Compare")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(crate::i18n::tf!("Compare {} with:", self.docs[index].name()));
                    ui.add_space(4.0);
                    for (other, name) in others.iter() {
                        if ui.button(name.as_str()).clicked() {
                            open_file = Some(*other);
                        }
                    }
                    from_disk = ui.button(crate::i18n::tr("A file on disk...")).clicked();
                    ui.add_space(4.0);
                    cancelled |= ui.button(crate::i18n::tr("Cancel")).clicked();
                });
                let this = self.docs[index].id;
                if let Some(other) = open_file {
                    self.close_dialog();
                    let other = self.docs[other].id;
                    self.open_file_compare_sides(compare_host::FileSide::Open(this), compare_host::FileSide::Open(other));
                } else if from_disk {
                    self.close_dialog();
                    if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Compare with")).pick_file() {
                        self.open_file_compare_sides(compare_host::FileSide::Open(this), compare_host::FileSide::Path(path));
                    }
                } else if cancelled {
                    self.close_dialog();
                }
            }
            Dialog::ConfirmReplaceInFiles { files, matches } => {
                let mut go = false;
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                if let Some(plan) = &self.replace_plan {
                    crate::fit::Dialog::new(crate::i18n::tr("Replace in Files")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                        ui.set_max_width(460.0);
                        ui.label(crate::i18n::tf!("Replace \"{}\" with \"{}\"?", plan.options.pattern, plan.options.replacement));
                        ui.label(if *matches == 1 { if files.len() == 1 { crate::i18n::tf!("{matches} match in {} file.", files.len()) } else { crate::i18n::tf!("{matches} match in {} files.", files.len()) } } else { if files.len() == 1 { crate::i18n::tf!("{matches} matches in {} file.", files.len()) } else { crate::i18n::tf!("{matches} matches in {} files.", files.len()) } });
                        ui.add_space(6.0);
                        ui.label(crate::i18n::tr("Files open here change in their windows, unsaved, and can be undone."));
                        ui.label(if plan.backup { crate::i18n::tr("The others are rewritten on disk now, each with a .bak copy of how it was.") } else { crate::i18n::tr("The others are rewritten on disk now, and that can't be undone.") });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            go = ui.button(crate::i18n::tr("Replace")).clicked();
                            cancelled |= ui.button(crate::i18n::tr("Cancel")).clicked();
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
                    Some(settings::Outcome::AmigaNdk) => self.open_ndk_box(),
                    Some(settings::Outcome::AmigaTools) => self.open_amiga_tools(),
                    Some(settings::Outcome::NewKinds) => self.open_new_kinds_box(),
                    Some(settings::Outcome::CompareRules) => self.open_rules_box(None),
                    Some(settings::Outcome::ToolbarButtons) => self.open_toolbar_box(),
                    Some(settings::Outcome::LanguageServers) => self.dialog = Dialog::LanguageServers(self.lsp.settings.clone()),
                    Some(settings::Outcome::SetFileTypes | settings::Outcome::RemoveFileTypes | settings::Outcome::OpenWindowsPage) | None => {}
                }
            }
            Dialog::ColumnRemove { cut, count, error, focus } => {
                let cut = *cut;
                let mut go = false;
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                crate::fit::Dialog::new(if cut { "Cut columns" } else { "Delete columns" }).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(crate::i18n::tf!("Number of columns to {} on every line, starting at this line and column.", if cut { "cut" } else { "delete" }));
                    ui.label(crate::i18n::tr("Leave it at 0 to use the highlighted columns instead."));
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
                        None => Some(crate::i18n::tr("That isn't a whole number of columns.").to_string()),
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
                crate::fit::Dialog::new(crate::i18n::tr("Convert to fixed width")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(crate::i18n::tf!("Lines {} to {}: each value is padded to its column's widest.", rows.0 + 1, rows.1 + 1));
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
                        Err(place) => *error = crate::i18n::tf!("Line {} has a quote that's never closed, so nothing was changed.", top + place + 1),
                    }
                }
            }
            Dialog::FixedToCsv { rows, delimiter, starts, error, focus } => {
                let mut go = false;
                let mut cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
                crate::fit::Dialog::new(crate::i18n::tr("Convert fixed width to CSV")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(crate::i18n::tf!("Lines {} to {}: the padding is removed from each value.", rows.0 + 1, rows.1 + 1));
                    ui.add_space(4.0);
                    ui.label(crate::i18n::tr("Columns start at (change these if a value got split):"));
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
                crate::fit::Dialog::new(crate::i18n::tr("Hex insert/delete")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.radio_value(delete, false, crate::i18n::tr("Insert"));
                        ui.radio_value(delete, true, crate::i18n::tr("Delete"));
                    });
                    ui.add_space(4.0);
                    ui.label(crate::i18n::tr("Number of bytes (use 0x for hex):"));
                    let field = ui.text_edit_singleline(count);
                    if *focus {
                        field.request_focus();
                        *focus = false;
                    }
                    ui.add_enabled_ui(!*delete, |ui| {
                        ui.label(crate::i18n::tr("Fill new bytes with this value, in hex:"));
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
                                Some(crate::i18n::tr("There are no bytes after the cursor to delete.").to_string())
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
                crate::fit::Dialog::new(crate::i18n::tr("Insert from another encoding")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(crate::i18n::tf!("Byte {byte:02X} in {} goes into a document in {}.", set.name, current.charset.name));
                    ui.add_space(4.0);
                    let label = meant.map_or_else(|| crate::i18n::tr("It has no character to convert").to_string(), |c| crate::i18n::tf!("Insert the character \"{c}\" ({})", charset::char_name(c)));
                    if ui.add_enabled(fits.is_some(), Button::new(label)).clicked() {
                        text = fits.map(String::from);
                    }
                    if ui.button(crate::i18n::tf!("Insert the raw byte {byte:02X}")).on_hover_text(crate::i18n::tr("Exactly this byte. It may mean something else in this document's encoding")).clicked() {
                        text = Some(char_table::text_for_byte(current.charset, byte));
                    }
                    if ui.button(crate::i18n::tr("Cancel")).clicked() {
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
                crate::fit::Dialog::new(crate::i18n::tr("Editor font")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.set_width(320.0);
                    ui.label(RichText::new(crate::i18n::tr("Columns depend on every character being the same width, so only fonts like that can be used.")).size(12.5).color(theme::text_dim()));
                    ui.add_space(4.0);
                    for (name, path) in installed_fonts() {
                        let on = current.as_deref().map_or(name == "Consolas", |file| same_path(file, &path));
                        if ui.add(Button::selectable(on, name)).clicked() {
                            chosen = Some(path);
                        }
                    }
                    ui.add_space(4.0);
                    browse = ui.button(crate::i18n::tr("Another font file...")).clicked();
                    if !error.is_empty() {
                        ui.label(RichText::new(error.as_str()).color(theme::rainbow_mistake()));
                    }
                    ui.add_space(4.0);
                    close |= ui.button(crate::i18n::tr("Close")).clicked();
                });
                if browse {
                    chosen = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Choose a font")).add_filter(crate::i18n::tr("Fonts"), &["ttf", "otf", "ttc"]).pick_file();
                }
                if let Some(path) = chosen {
                    match std::fs::read(&path) {
                        Ok(bytes) if is_fixed_pitch(bytes.clone()) => {
                            install_fonts(ctx, Some(&path));
                            self.font_file = Some(path);
                        }
                        Ok(_) => {
                            if let Dialog::Fonts { error } = &mut self.dialog {
                                *error = crate::i18n::tr("That font can't be used: its characters aren't all the same width, or it can't be read.").to_string();
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
            Dialog::WindowFont { error } => {
                let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
                let (mut chosen, mut browse, mut usual) = (None, false, false);
                let current = WINDOW_FONT.lock().ok().and_then(|f| f.clone());
                let mut scale = self.window_scale;
                crate::fit::Dialog::new(crate::i18n::tr("Window font and size")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.set_width(340.0);
                    ui.label(RichText::new(crate::i18n::tr("For the menus, panels, boxes and the status bar. The text's own font and size are under View, Font, and Ctrl and + or -.")).size(12.5).color(theme::text_dim()));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr("Size"));
                        ui.add(egui::Slider::new(&mut scale, WINDOW_SCALES).step_by(0.05).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                        if ui.small_button(crate::i18n::tr("Usual")).clicked() {
                            scale = 1.0;
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(crate::i18n::tr("Font"));
                    if ui.add(Button::selectable(current.is_none(), crate::i18n::tr("The usual one"))).clicked() {
                        usual = true;
                    }
                    for (name, path) in installed_window_fonts() {
                        let on = current.as_deref().is_some_and(|file| same_path(file, &path));
                        if ui.add(Button::selectable(on, name)).clicked() {
                            chosen = Some(path);
                        }
                    }
                    browse = ui.button(crate::i18n::tr("Another font file...")).clicked();
                    if !error.is_empty() {
                        ui.label(RichText::new(error.as_str()).color(theme::rainbow_mistake()));
                    }
                    ui.add_space(4.0);
                    close |= ui.button(crate::i18n::tr("Close")).clicked();
                });
                if (scale - self.window_scale).abs() > 0.001 {
                    // The slider's still under the pointer while it's dragged: the window's resized when let go.
                    if !ctx.input(|i| i.pointer.any_down()) {
                        self.window_scale = scale;
                        self.save_settings_if_changed();
                    } else {
                        self.window_scale = scale;
                    }
                }
                if browse {
                    chosen = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Choose a font")).add_filter(crate::i18n::tr("Fonts"), &["ttf", "otf", "ttc"]).pick_file();
                }
                if usual || chosen.is_some() {
                    let ok = chosen.as_ref().is_none_or(|path| std::fs::read(path).is_ok());
                    if ok {
                        if let Ok(mut window) = WINDOW_FONT.lock() {
                            *window = chosen;
                        }
                        install_fonts(ctx, self.font_file.as_deref());
                        self.save_settings_if_changed();
                    } else if let Dialog::WindowFont { error } = &mut self.dialog {
                        *error = crate::i18n::tr("That font can't be read.").to_string();
                    }
                }
                if close {
                    self.close_dialog();
                }
            }
            Dialog::Color { rgb, form } => {
                let mut insert = None;
                let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
                crate::fit::Dialog::new(crate::i18n::tr("Insert color")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        egui::color_picker::color_edit_button_srgb(ui, rgb);
                        ui.label(RichText::new(tools::color_text(*rgb, *form)).monospace());
                    });
                    ui.add_space(4.0);
                    for (index, name) in tools::COLOR_FORMS.iter().enumerate() {
                        ui.radio_value(form, index, crate::i18n::tr(name));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(crate::i18n::tr("Insert")).clicked() {
                            insert = Some(tools::color_text(*rgb, *form));
                        }
                        close |= ui.button(crate::i18n::tr("Cancel")).clicked();
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
        let mut pick = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
            for i in 0..if self.no_file() { 0 } else { self.docs.len() } {
                let numbers = self.docs[i].window_numbers();
                let several = numbers.len() > 1;
                for number in numbers {
                    let doc = &self.docs[i];
                    let name = if several { format!("{}:{number}", doc.name()) } else { doc.name() };
                    let name = if doc.merge.is_some() { format!("Merge: {name}") } else { name };
                    let (file_name, modified, selected) = (doc.name(), doc.is_modified(), i == self.active && number == doc.window_number);
                    let stripe = self.doc_look(i).tint.map(crate::file_colours::Tint::mark);
                    let (chosen, closing, response) = tabs::file_tab(ui, &name, &file_name, modified, selected, stripe);
                    crate::fit::context_menu(&response, |ui| {
                        if let Some(choice) = self.tab_menu(ui, i) {
                            pick = Some((i, choice));
                        }
                    });
                    if chosen {
                        self.choose_tab_window(i, number);
                    }
                    if closing {
                        close = Some((i, number));
                    }
                }
            }
        });
        self.finish_tab_actions(pick, close);
    }

    fn status_bar(&self, ui: &mut Ui) -> (Option<MenuAction>, bool) {
        if self.no_file() {
            ui.label(RichText::new(crate::i18n::tr("No file open. Open a file, or a folder of code (File, Open folder), or drop one here.")).color(theme::text_faint()));
            return (None, false);
        }
        if let Some(tab) = &self.docs[self.active].compare {
            ui.label(RichText::new(tab.status_words()).color(theme::text_faint()));
            return (None, false);
        }
        let doc = &self.docs[self.active];
        let mut action = None;
        let mut toggle_lock = false;
        let faint = |words: String| RichText::new(crate::i18n::translation(&words).map_or(words, str::to_string)).color(theme::text_faint());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            if scripting::recording() {
                ui.colored_label(theme::rainbow_mistake(), crate::i18n::tr("\u{25CF} Recording a macro")).on_hover_text(crate::i18n::tr("Choose Tools, Macros, Record or stop, or press Ctrl+Shift+R, when you're done"));
            }
            if let Some(source) = &doc.scanning {
                ui.colored_label(theme::accent(), crate::i18n::tf!("Finding lines: {:.0}%", source.progress() * 100.0)).on_hover_text(crate::i18n::tf!("{} lines so far. You can read and move around now, and editing starts when this finishes", doc.text.line_count()));
            }
            if let Some(job) = &doc.saving {
                let share = job.written.load(Ordering::Relaxed) as f32 / job.total.max(1) as f32;
                ui.colored_label(theme::accent(), crate::i18n::tf!("Saving: {:.0}%", (share * 100.0).min(100.0)));
            }
            if let Some(status) = self.remote_status() {
                ui.colored_label(theme::accent(), status);
            }
            if let Some((status, clickable)) = self.remote_project_status() {
                let label = ui.add(egui::Label::new(RichText::new(status).color(theme::accent())).sense(if clickable { egui::Sense::click() } else { egui::Sense::hover() }));
                if clickable && label.on_hover_text(crate::i18n::tr("Click to sync with the server, or to connect again")).clicked() {
                    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(remote_project_host::STATUS_CLICKED), true));
                }
            }
            if let Some(blame) = &self.git.blame_line {
                ui.label(faint(blame.clone()));
            }
            if let Some(view) = &doc.live {
                let words = match &view.problem {
                    Some(problem) => problem.clone(),
                    None => crate::i18n::tf!("Following the {} as it runs", view.machine.name()),
                };
                ui.colored_label(theme::accent(), words);
            }
            self.database_status(ui);
            self.http_status(ui);
            if let Some(status) = self.run_status() {
                ui.colored_label(theme::accent(), status);
                if !self.running.sessions.is_empty() && ui.small_button(crate::i18n::tr("Stop")).on_hover_text(crate::i18n::tr("Stops the program running, closing the emulator")).clicked() {
                    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(run_host::STOP_ASKED), true));
                }
            }
            if let Some((status, more)) = self.send_status() {
                let label = ui.colored_label(theme::accent(), status);
                if !more.is_empty() {
                    label.on_hover_text(more);
                }
                if !self.sends.jobs.is_empty() && ui.small_button(crate::i18n::tr("Stop")).on_hover_text(crate::i18n::tr("Stop sending. Nothing half-sent is left on a card or folder")).clicked() {
                    for job in &self.sends.jobs {
                        job.stop();
                    }
                }
            }
            if let Some(job) = &self.big_job {
                ui.colored_label(theme::accent(), crate::i18n::tf!("{}: {:.0}%", job.title, (job.share() * 100.0).min(100.0)));
                if job.title == "Query the rows" && ui.small_button(crate::i18n::tr("Stop")).on_hover_text(crate::i18n::tr("Stop the query")).clicked() {
                    job.cancel();
                }
            }
            // Only shown when something's wrong: if edits aren't being kept, the person needs to know.
            let kept = self.recovery_failed.clone().or_else(|| self.recovery.as_ref().and_then(recovery::Session::problem)).filter(|_| self.screenshot.is_none());
            if let Some(problem) = kept {
                ui.colored_label(theme::rainbow_mistake(), crate::i18n::tr("Edits aren't being kept for recovery. Save often.")).on_hover_text(problem);
            }
            if self.status_layout.shows(status_items::Item::Position) {
                ui.label(if doc.hex_mode { doc.hex.position_text() } else { doc.text.position_text() });
            }
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
                        if let Some(words) = self.selection_cycles() {
                            ui.label(faint(words));
                        }
                    }
                }
            }
            if !self.print_status.is_empty() {
                ui.label(faint(self.print_status.clone()));
            }
            if let Some(why) = doc.autosave_skipped.filter(|_| self.autosave_mode().is_some()) {
                ui.label(faint(crate::i18n::tr("Not saved by itself").to_string())).on_hover_text(crate::i18n::tf!("Autosave is leaving this file for you to save: {why}."));
            }
            if let Some(filter) = &doc.filter {
                ui.colored_label(theme::accent(), crate::i18n::tf!("Showing lines with \"{}\": {} of {}", filter.needle, filter.lines.len(), doc.text.line_count()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 18.0;
                let (modified, size, read_only) = doc.status_file_parts();
                // The notes that come and go by themselves, at the far right.
                if !doc.hex_mode && doc.text.column_mode() {
                    ui.colored_label(theme::accent(), crate::i18n::tr("Column mode"));
                }
                let overwriting = if doc.hex_mode { !doc.hex.insert_mode } else { doc.text.overwrite };
                if overwriting {
                    ui.colored_label(theme::accent(), crate::i18n::tr("Overwriting"));
                }
                if let Some(zx) = doc.text.zx.as_ref().filter(|_| !doc.hex_mode) {
                    if zx.graph {
                        ui.label(RichText::new(crate::i18n::tr("GRAPH")).strong().color(theme::accent())).on_hover_text(crate::i18n::tr("GRAPH mode, as on the Spectrum: 1 to 8 type block graphics, with Shift the inverted ones, and A to U type UDGs. 9, Escape or Ctrl+Shift+9 leaves it"));
                    }
                    if let Some(from) = zx.udgs_from.as_ref().filter(|_| zx.has_pictures()) {
                        ui.label(faint(crate::i18n::tr("UDG pictures").into())).on_hover_text(crate::i18n::tf!("UDGs are drawn as they look, with the pictures from {from}. Put the cursor in one to edit it as text"));
                    }
                }
                if crate::clipboards::active() != 0 {
                    ui.label(faint(crate::clipboards::name(crate::clipboards::active()))).on_hover_text(crate::i18n::tr("Copy, Cut and Paste use this clipboard, one of Limoniter's own. Ctrl+0 goes back to the computer's"));
                }
                let can_edit = !doc.is_busy() && !doc.hex_mode && self.pending.is_none();
                let format = &doc.text.format;
                let remembered = doc.path.as_deref().is_some_and(|path| self.encoding_prefs.has(path));
                // Then the steady items, in the order chosen.
                for item in self.status_layout.right_to_left() {
                    match item {
                        status_items::Item::Lock => {
                            let locked = read_only || doc.text.read_only;
                            if doc.browser.as_ref().is_some_and(|b| b.writable()) {
                                // An image whose files can be changed: each change is saved as it's made, so there's no lock.
                                ui.label(faint(crate::i18n::tr("Changes save at once").into())).on_hover_text(crate::i18n::tr("Files added, deleted or renamed on this image are saved to it straight away; Undo puts it back"));
                            } else {
                                let lock = RichText::new(if locked { crate::i18n::tr("Read-only") } else { crate::i18n::tr("Editable") }).color(if locked { theme::change_unsaved() } else { theme::text_faint() });
                                let lock = ui.add(egui::Label::new(lock).sense(egui::Sense::click()));
                                crate::access::chosen(&lock, egui::Role::CheckBox, locked, "Read-only");
                                if lock.on_hover_text(crate::i18n::tr("Click to lock this file against changes, or unlock it")).clicked() {
                                    toggle_lock = true;
                                }
                            }
                        }
                        status_items::Item::Size => {
                            let size_label = ui.label(faint(size.clone()));
                            if let Some(modified) = &modified {
                                size_label.on_hover_text(modified.clone());
                            }
                        }
                        status_items::Item::Profile => {
                            if !doc.hex_mode && (format.display != Display::Plain || format.charset.keep_bytes) {
                                let shown = profile_status_name(format);
                                let profile = encoding_ui::status_menu(ui, shown, "The active display profile", |ui| encoding_ui::display_menu(ui, format, self.view.source_view));
                                action = action.or(profile);
                            }
                        }
                        status_items::Item::LineEndings => {
                            if !doc.hex_mode {
                                let counts = doc.text.eol_counts();
                                let name = eol_status_name(format, &counts, doc.text.insert_eol());
                                let on_disk = doc.text.is_disk_backed();
                                let eol = encoding_ui::status_menu(ui, name, &counts.describe(), |ui| encoding_ui::eol_menu(ui, &counts, doc.text.insert_eol(), format, can_edit, on_disk));
                                action = action.or(eol);
                            }
                        }
                        status_items::Item::Encoding => {
                            let encoding = encoding_ui::status_menu(ui, format.encoding_name(), format.certainty.describe(), |ui| encoding_ui::encoding_menu(ui, format, remembered, can_edit));
                            action = action.or(encoding);
                        }
                        status_items::Item::Language => {
                            if doc.hex_mode {
                                ui.label(if doc.hex.ebcdic { crate::i18n::tr("Hex, EBCDIC") } else { crate::i18n::tr("Hex") });
                            } else {
                                if doc.text.is_disk_backed() {
                                    ui.label(faint(crate::i18n::tr("Read from disk as needed").into())).on_hover_text(crate::i18n::tr("This file is too big to hold in memory, so it's read a part at a time"));
                                }
                                // A click chooses another (Colour as).
                                if ui.add(egui::Label::new(crate::i18n::tr(&doc.language_name())).sense(egui::Sense::click())).on_hover_text(crate::i18n::tr("Colour as another language")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(language_picker::OPEN_ASKED), true));
                                }
                                // A Python file's environment, beside its language.
                                self.python_status(ui);
                            }
                        }
                        status_items::Item::Position => {}
                    }
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
                ui.label(RichText::new(crate::i18n::tr("Find and replace")).strong().size(14.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = crate::access::called(ui.add(Button::new(RichText::new(crate::i18n::tr("\u{D7}")).color(theme::text_dim())).frame(false)), "Close").on_hover_text(crate::i18n::tr("Close")).clicked();
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
            // The rest scrolls when the window is too short for it all.
            egui::ScrollArea::vertical().id_salt("find-panel-scroll").auto_shrink([false, true]).show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Find what:"));
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
                    ui.label(crate::i18n::tr("Replace with:"));
                    ui.add(egui::TextEdit::singleline(&mut self.find.replace).desired_width(f32::INFINITY));
                    ui.add_space(6.0);
                }
                if in_files_tab {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(crate::i18n::tr("In:"));
                        ui.radio_value(&mut self.find.scope, FileScope::Folder, crate::i18n::tr("Files in a folder"));
                        ui.radio_value(&mut self.find.scope, FileScope::OpenFiles, crate::i18n::tr("Open files"));
                        ui.radio_value(&mut self.find.scope, FileScope::Favorites, crate::i18n::tr("Favorite files"));
                        let has_project = self.project.is_some();
                        if !has_project && self.find.scope == FileScope::Project {
                            self.find.scope = FileScope::Folder;
                        }
                        ui.add_enabled_ui(has_project, |ui| ui.radio_value(&mut self.find.scope, FileScope::Project, crate::i18n::tr("Project files"))).response.on_disabled_hover_text(crate::i18n::tr("Open a project from Tools, Project first"));
                    });
                    ui.add_space(6.0);
                    if self.find.scope == FileScope::Favorites {
                        let mut gone = None;
                        if self.find.favorite_files.is_empty() {
                            ui.label(RichText::new(crate::i18n::tr("No favorite files yet.")).color(theme::text_dim()));
                        }
                        for (place, path) in self.find.favorite_files.iter().enumerate() {
                            ui.horizontal(|ui| {
                                if crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Take off the list").on_hover_text(crate::i18n::tr("Take off the list")).clicked() {
                                    gone = Some(place);
                                }
                                let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
                                ui.label(crate::i18n::tr(&name)).on_hover_text(path.display().to_string());
                            });
                        }
                        if let Some(place) = gone {
                            self.find.favorite_files.remove(place);
                        }
                        ui.horizontal(|ui| {
                            let this_file = self.docs[self.active].path.clone();
                            if ui.add_enabled(this_file.is_some(), Button::new(crate::i18n::tr("Add this file"))).clicked()
                                && let Some(path) = this_file
                                && !self.find.favorite_files.contains(&path)
                            {
                                self.find.favorite_files.push(path);
                            }
                            if ui.button(crate::i18n::tr("Add files...")).clicked()
                                && let Some(paths) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Add favorite files")).pick_files()
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
                        ui.label(crate::i18n::tr("In files/types:"));
                        ui.add(egui::TextEdit::singleline(&mut self.find.file_types).desired_width(f32::INFINITY).hint_text(crate::i18n::tr("*.txt; *.log")));
                        ui.add_space(4.0);
                        ui.label(crate::i18n::tr("Directory:"));
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.find.folder).desired_width(ui.available_width() - 34.0));
                            if ui.button(crate::i18n::tr("...")).on_hover_text(crate::i18n::tr("Choose a folder")).clicked()
                                && let Some(folder) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Search in folder")).pick_folder()
                            {
                                self.find.folder = folder.display().to_string();
                            }
                        });
                        ui.checkbox(&mut self.find.recursive, crate::i18n::tr("Include subfolders"));
                        ui.add_space(4.0);
                    }
                }
                ui.checkbox(&mut self.find.match_case, crate::i18n::tr("Match case"));
                if hex && !in_files_tab {
                    ui.checkbox(&mut self.find.hex_bytes, crate::i18n::tr("Hex bytes"));
                } else {
                    ui.checkbox(&mut self.find.whole_word, crate::i18n::tr("Whole word"));
                    ui.checkbox(&mut self.find.regex, crate::i18n::tr("Regular expression"));
                    if !in_files_tab {
                        let known = crate::tools::comment_marks(&self.docs[self.active].language_name()).is_some();
                        ui.add_enabled(known, egui::Checkbox::new(&mut self.find.code_only, crate::i18n::tr("Not in comments or strings")))
                            .on_hover_text(crate::i18n::tr("Matches in the code alone count"))
                            .on_disabled_hover_text(crate::i18n::tr("The comments of this file's language aren't known"));
                    }
                }
                if self.find.tab == FindTab::ReplaceInFiles {
                    ui.checkbox(&mut self.find.keep_backups, crate::i18n::tr("Keep a .bak copy of each file changed on disk"));
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| match self.find.tab {
                    FindTab::Find => {
                        next = ui.button(crate::i18n::tr("Find Next")).clicked() || entered;
                        previous = ui.button(crate::i18n::tr("Find Previous")).clicked();
                        list_lines = ui.add_enabled(!hex, Button::new(crate::i18n::tr("List lines"))).on_hover_text(crate::i18n::tr("Lists each line containing the find string, in the results below")).clicked();
                    }
                    FindTab::Replace => {
                        next = ui.button(crate::i18n::tr("Find Next")).clicked() || entered;
                        replace = ui.button(crate::i18n::tr("Replace")).clicked();
                        replace_all = ui.button(crate::i18n::tr("Replace All")).clicked();
                    }
                    FindTab::InFiles => {
                        in_files = ui.button(crate::i18n::tr("Find")).clicked() || entered;
                    }
                    FindTab::ReplaceInFiles => {
                        preview = ui.button(crate::i18n::tr("Preview")).on_hover_text(crate::i18n::tr("Lists every match. Nothing changes until you go ahead from the results")).clicked() || entered;
                    }
                });
                if !self.find.status.is_empty() {
                    ui.add_space(6.0);
                    ui.label(&self.find.status);
                }
            });
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
                    ui.label(crate::i18n::tr("Output window: results from Find in Files appear here."));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Close").on_hover_text(crate::i18n::tr("Close")).clicked();
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
                let mut summary = {
                    let matches = if hits.len() == 1 { crate::i18n::tr("1 match").to_string() } else { crate::i18n::tf!("{} matches", hits.len()) };
                    let files = if searched == 1 { crate::i18n::tr("1 file").to_string() } else { crate::i18n::tf!("{searched} files") };
                    if finished { crate::i18n::tf!("{title} finished: {matches} in {files}") } else { crate::i18n::tf!("{title} searching: {matches} in {files}") }
                };
                if job.truncated.load(Ordering::Relaxed) {
                    summary += &format!(" (stopped at {})", find_files::MAX_HITS);
                }
                if unreadable > 0 {
                    summary += &crate::i18n::tf!(", {unreadable} couldn't be read");
                }
                ui.label(summary);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Close").on_hover_text(crate::i18n::tr("Close")).clicked();
                    if !finished && ui.small_button(crate::i18n::tr("Stop")).clicked() {
                        job.cancel.store(true, Ordering::Relaxed);
                    }
                    if ready_to_replace {
                        ask_to_replace = ui.button(crate::i18n::tr("Replace these...")).on_hover_text(crate::i18n::tr("Asks first")).clicked();
                    }
                });
            });
            ui.separator();
            let row = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
            egui::ScrollArea::both().id_salt("results").auto_shrink([false, false]).show_rows(ui, row, hits.len(), |ui, range| {
                for hit in &hits[range] {
                    let label = format!("{}({}): {}", hit.path.display(), hit.line + 1, hit.text);
                    let text = RichText::new(label).monospace();
                    let row = ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend).sense(egui::Sense::click()));
                    crate::access::button(&row, &format!("{}, line {}: {}", hit.path.display(), hit.line + 1, hit.text));
                    if row.clicked() {
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
        if self.palette_dialog(ctx) || self.toolbar_dialog(ctx) || self.addons_dialog(ctx) || self.extensions_dialog(ctx) || self.remote_dialog(ctx) || self.theme_dialog(ctx) || self.send_dialog(ctx) || self.devices_dialog(ctx) || self.run_dialog(ctx) || self.assembler_dialog(ctx) || self.compilers_dialog(ctx) || self.new_kinds_dialog(ctx) || self.rename_dialog(ctx) || self.query_rows_dialog(ctx) || self.disassemble_dialog(ctx) || self.new_image_dialog(ctx) || self.web_dialog(ctx) || self.web_project_dialog(ctx) || self.c64_tools_dialog(ctx) || self.ndk_dialog(ctx) || self.amiga_project_dialog(ctx) || self.database_dialog(ctx) || self.amiga_tools_dialog(ctx) || self.amiga_icon_dialog(ctx) || self.st_tools_dialog(ctx) || self.language_servers_dialog(ctx) || self.tasks_dialog(ctx) || self.git_history_dialog(ctx) || self.git_wizard_dialog(ctx) || self.script_dialog(ctx) || self.file_tool_dialogs(ctx) || self.assistant_dialogs(ctx) || self.recovery_dialog(ctx) || self.update_dialog(ctx) || self.encoding_dialog(ctx) || self.node_dialog(ctx) || self.templates_dialog(ctx) || self.compare_dialogs(ctx) {
            return;
        }
        let mut close = false;
        let mut goto = None;
        let mut save_basic = None;
        let mut fill = None;
        let mut number = None;
        let mut show_features = false;
        let mut show_licence: Option<PathBuf> = None;
        let mut surround_tag = None;
        let mut wrap_abbreviation = None;
        let mut save_layout = None;
        let mut column_without_wrap = false;
        let title = match &self.dialog {
            Dialog::None => return,
            Dialog::GoTo { .. } => crate::i18n::tr("Go To").to_string(),
            Dialog::Fill { .. } => crate::i18n::tr("Insert or fill columns").to_string(),
            Dialog::SaveBasic { .. } => crate::i18n::tr("Save as a BASIC program").to_string(),
            Dialog::Number { .. } => crate::i18n::tr("Insert Number").to_string(),
            Dialog::Message { title, .. } => title.clone(),
            Dialog::ColumnNeedsNoWrap => crate::i18n::tr("Column mode").to_string(),
            Dialog::SaveLayout { .. } => crate::i18n::tr("Save this layout").to_string(),
            Dialog::SurroundTag { .. } => crate::i18n::tr("Surround with a tag").to_string(),
            Dialog::WrapAbbreviation { .. } => crate::i18n::tr("Wrap with an abbreviation").to_string(),
            Dialog::About { .. } => crate::i18n::tr("About Limoniter").to_string(),
            _ => return,
        };
        let hex = self.docs[self.active].hex_mode;
        let enter = ctx.input(|i| i.key_pressed(Key::Enter));
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }
        crate::fit::Dialog::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| match &mut self.dialog {
            Dialog::None => {}
            Dialog::GoTo { value, error } => {
                ui.label(if hex { crate::i18n::tr("Offset (decimal, or 0x for hex):") } else { crate::i18n::tr("Line number:") });
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
                        ui.label(crate::i18n::tr("Which machine is it for?"));
                        for choice in basic::CbmMachine::ALL {
                            ui.radio_value(cbm, choice, crate::i18n::tr(choice.name()));
                        }
                    }
                    _ => {
                        ui.label(crate::i18n::tr("Run it from line (leave this empty for a program that doesn't start by itself):"));
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
                ui.label(crate::i18n::tr("Text to put on every line, starting at this line and column:"));
                ui.text_edit_singleline(value).request_focus();
                ui.checkbox(skip_last, crate::i18n::tr("Leave an empty last line alone"));
                let (ok, cancel) = ok_cancel(ui);
                close |= cancel;
                if ok || enter {
                    fill = Some((value.clone(), *skip_last));
                }
            }
            Dialog::Number { first, step, error } => {
                egui::Grid::new("number-grid").num_columns(2).show(ui, |ui| {
                    ui.label(crate::i18n::tr("First number:"));
                    ui.text_edit_singleline(first);
                    ui.end_row();
                    ui.label(crate::i18n::tr("Add each line:"));
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
            Dialog::SurroundTag { value, error } => {
                ui.label(crate::i18n::tr("The tag, with any attributes (p, or div class=\"note\"):"));
                ui.text_edit_singleline(value).request_focus();
                if !error.is_empty() {
                    ui.label(error.as_str());
                }
                let (ok, cancel) = ok_cancel(ui);
                close |= cancel;
                if ok || enter {
                    surround_tag = Some(value.clone());
                }
            }
            Dialog::WrapAbbreviation { value, error } => {
                ui.label(crate::i18n::tr("An Emmet abbreviation (ul>li* makes a list item of each line):"));
                ui.text_edit_singleline(value).request_focus();
                if !error.is_empty() {
                    ui.label(error.as_str());
                }
                let (ok, cancel) = ok_cancel(ui);
                close |= cancel;
                if ok || enter {
                    wrap_abbreviation = Some(value.clone());
                }
            }
            Dialog::SaveLayout { name, error } => {
                ui.label(crate::i18n::tr("A name for the panels open now, such as Writing or Debugging:"));
                ui.text_edit_singleline(name).request_focus();
                if !error.is_empty() {
                    ui.label(error.as_str());
                }
                let (ok, cancel) = ok_cancel(ui);
                close |= cancel;
                if ok || enter {
                    save_layout = Some(name.clone());
                }
            }
            Dialog::Message { body, .. } => {
                ui.label(body.as_str());
                ui.add_space(6.0);
                close |= ui.button(crate::i18n::tr("OK")).clicked() || enter;
            }
            Dialog::ColumnNeedsNoWrap => {
                ui.label(crate::i18n::tr("Column mode can't be used with Word wrap on."));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    column_without_wrap = ui.button(crate::i18n::tr("Turn off Word wrap")).on_hover_text(crate::i18n::tr("Turns Word wrap off, for every document, and Column mode on")).clicked() || enter;
                    close |= ui.button(crate::i18n::tr("Cancel")).clicked();
                });
            }
            Dialog::About { notices } => {
                let (ok, features, licence) = about_page(ui, notices.as_deref());
                close |= ok || enter;
                show_features |= features;
                if licence {
                    show_licence = notices.clone();
                }
            }
            _ => {}
        });

        if show_features {
            close = true;
            self.open_features();
        }
        // The licences open in a tab of their own, to read rather than change.
        if let Some(path) = show_licence {
            close = true;
            if self.open_path(&path) {
                self.doc().set_read_only(true);
            }
        }
        if column_without_wrap {
            self.dialog = Dialog::None;
            if self.view.word_wrap {
                self.toggle_word_wrap();
            }
            self.doc().text.set_column_mode(true);
        }
        if let Some(name) = save_layout {
            let name = name.trim().to_string();
            if name.is_empty() {
                self.dialog = Dialog::SaveLayout { name, error: crate::i18n::tr("Type a name for it.").into() };
            } else {
                let open = self.layout_now();
                crate::layouts::keep(&mut self.layouts, crate::layouts::Layout { name, open });
                close = true;
            }
        }
        if let Some(typed) = wrap_abbreviation {
            self.dialog = Dialog::None;
            if let Err(problem) = self.wrap_with_abbreviation(&typed) {
                self.dialog = Dialog::WrapAbbreviation { value: typed, error: problem };
            }
        }
        if let Some(typed) = surround_tag {
            self.dialog = Dialog::None;
            if let Err(problem) = self.surround_with_tag(&typed) {
                self.dialog = Dialog::SurroundTag { value: typed, error: problem };
            }
        }
        if let Some(value) = goto {
            let doc = self.doc();
            let found = if doc.hex_mode { doc.hex.go_to(&value) } else { doc.text.go_to_numbered(&value) };
            if found {
                scripting::note_line(format!("go_to_place({});", scripting::quoted(&value)));
                close = true;
            } else if let Dialog::GoTo { error, .. } = &mut self.dialog {
                *error = crate::i18n::tf!("\"{value}\" isn't a valid position.");
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
                        *error = crate::i18n::tr("A line to run from is a number from 0 to 9999.").to_string();
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
                        crate::i18n::tr("Those numbers get too large to count.").to_string()
                    }
                }
                _ => crate::i18n::tr("Please enter whole numbers.").to_string(),
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
            // Whether the built-in Spectrum shows its keyboard is kept by the window, not the app.
            if let Some(shown) = step.strip_prefix("spectrum-keyboard:") {
                ctx.data_mut(|d| d.insert_persisted(egui::Id::unique("built-in-spectrum-keyboard"), shown != "off"));
                return;
            }
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
        // A picture shows text in its colours, so it waits for the grammars too.
        highlight::engine().is_none()
            || self.debug_busy_for_picture()
            || self.big_job.is_some() || self.retro_pictures.as_ref().is_some_and(|c| c.working()) || self.docs.iter().any(|doc| doc.is_busy() || doc.table.as_deref().is_some_and(|table| table.working()) || doc.notebook.as_deref().is_some_and(|n| n.busy()) || doc.data.as_deref().is_some_and(|d| d.busy())) || self.database.busy()
    }

    /// A picture can show the app partway through something: LIMONITER_SHOT_STEPS lists what to do first. Steps
    /// with a value follow it after a colon.
    fn screenshot_step(&mut self, step: &str) {
        if let Some((name, value)) = step.split_once(':') {
            match name {
                "open-entry" => self.open_from_browser(self.active, value.parse().unwrap_or(0)),
                // The newer-version window at a stage: offered, downloading, ready or failed.
                "update" => self.update_screenshot_step(value),
                // Debugging in Copperline (see `dap_host`): the program in front run there with a breakpoint, and
                // then the Debug panel's memory, profile, warnings and Copperline's own windows.
                "copperline-debug" | "debug-memory" | "debug-profile" | "debug-warnings" | "debug-fold" | "copperline-window" => self.copperline_screenshot_step(name, value),
                // The preview written to a PDF at a path, with no box to choose where.
                "save-pdf" => self.diagram_screenshot_step(name, value),
                // The music in front played from so many seconds in, and paused there so the picture is still.
                "play-music" => self.play_music_at(value.parse().unwrap_or(0.0)),
                // File properties shown, as Info shows it beside an image.
                "properties" => self.panels.file_properties = value != "off",
                // The file in front in hex with the Structure panel open, the cursor at an offset.
                "structure" => {
                    if !self.docs[self.active].hex_mode {
                        self.set_view(Some(false));
                    }
                    self.panels.structure = true;
                    let at = crate::hex_buffer::parse_number(value).unwrap_or(0);
                    self.doc().hex.move_caret(at, false);
                }
                // Files waiting to go on the image in front, as if dropped on it; semicolons between.
                "extensions" | "icon-theme" => self.extensions_screenshot_step(name, value),
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
                "next-painter" => {
                    // A Next screen in the painter, or a picture made into one by the converter; ";tool" after it.
                    let (path, tool) = value.split_once(';').unwrap_or((value, ""));
                    let bytes = std::fs::read(path).unwrap_or_default();
                    let screen = crate::next_screen::NextScreen::read(path, &bytes).ok().or_else(|| {
                        let image = crate::quantise::read_picture(path, &bytes).ok()?;
                        let settings = crate::retro_convert::Settings { mode: crate::retro_convert::Mode::Layer2, ..Default::default() };
                        crate::retro_convert::convert(&image, &settings).next_screen()
                    });
                    let mut painter = crate::next_painter::Painter::new(screen.unwrap_or_else(|| crate::next_screen::NextScreen::new(crate::next_screen::NextMode::Layer2)), "picture");
                    if tool == "text" {
                        painter.tool = crate::next_painter::Tool::Text;
                    }
                    self.next_painter = Some(Box::new(painter));
                }
                "atari2600-graphics" => {
                    // The 2600 graphics editor on its sprites or playfield; ";picture" after the playfield brings one in.
                    let (part, picture) = value.split_once(';').unwrap_or((value, ""));
                    let mut editor = crate::atari2600_painter::Editor::default();
                    if part == "playfield" {
                        editor.part = crate::atari2600_painter::Part::Playfield;
                        if let Ok(bytes) = std::fs::read(picture)
                            && let Ok(image) = crate::quantise::read_picture(picture, &bytes)
                        {
                            let f = &editor.field;
                            editor.field = crate::atari2600_gfx::Playfield::from_picture(&image, editor.tv, f.halves, f.rows, f.lines, editor.fit, false);
                        }
                    }
                    self.atari2600_graphics = Some(Box::new(editor));
                }
                // The AutoDoc reader, at a page found from a word (AllocMem), or with a filter ("filter:mem").
                // "autodocs-ndk" uses the NDK in a folder first, and "autodocs-mui" MUI's autodocs in a folder.
                "autodocs-ndk" => {
                    let _ = crate::amiga_ndk::set_folder(Some(Path::new(value)));
                }
                "autodocs-mui" => {
                    let _ = crate::autodocs::choose_mui(Some(Path::new(value)));
                }
                "autodocs" => match value.strip_prefix("filter:") {
                    Some(filter) => {
                        self.open_autodocs(None);
                        if let Some(reader) = &mut self.autodoc_reader {
                            reader.filter = filter.to_string();
                        }
                    }
                    None => self.open_autodocs(Some(value.to_string())),
                },
                // The PICO-8 and TIC-80 cart window, on the file in front, at a tab: sprites, map, sounds or parts.
                "cart-view" => {
                    self.open_cart_view();
                    if let Some(view) = &mut self.cart_view {
                        view.show_tab(value);
                    }
                }
                "raster-designer" => {
                    // The raster and copper designer for a machine (by its name, as "C64"), its start design.
                    let machine = crate::raster_designer::Machine::ALL.into_iter().find(|m| format!("{m:?}") == value).unwrap_or(crate::raster_designer::Machine::Amiga);
                    self.demo_tools.raster = Some(Box::new(crate::raster_designer::Designer::for_machine(machine)));
                }
                "assembly-tests" => {
                    // The tests of the source in front, its language found first, as it is a moment after opening.
                    highlight::load_now();
                    self.doc().update_syntax();
                    self.assembly_tests_here();
                }
                "sound-effects" => {
                    // The sound effects designer for a machine (by its name, as "C64Pal"), with every preset.
                    let mut editor = crate::sfx_designer::Editor::default();
                    let machine = crate::sfx::Machine::ALL.into_iter().find(|m| format!("{m:?}") == value).unwrap_or_default();
                    editor.bank = crate::sfx::Bank { machine, effects: crate::sfx::Preset::ALL.iter().map(|&p| crate::sfx::Effect::from_shape(p.name(), machine, &crate::sfx::Shape::preset(p))).collect() };
                    editor.chosen = 1;
                    editor.play_changes = false;
                    self.sound_effects = Some(Box::new(editor));
                }
                "project-settings" => {
                    // The project's settings box on a tab (its number, 0 to 6), for the picture.
                    self.open_project_settings();
                    if let Dialog::ProjectSettings(draft) = &mut self.dialog {
                        draft.tab = value.parse().unwrap_or(0).min(project_settings_host::TABS.len() - 1);
                    }
                }
                "recency" => {
                    // The Explorer's recency slider set to a share, with every file's date read now, for the picture.
                    if let Some(project) = &mut self.project
                        && let Some(root) = project.root.clone()
                    {
                        project.explorer.recency = value.parse().unwrap_or(100);
                        let times = crate::explorer_kept::every_time(&root, &project.files);
                        self.panels.project_panel.explorer.set_every_time(&root, times);
                    }
                }
                "open-folder" => {
                    // A folder opened as a project, with Go to file open on some letters ("path;letters").
                    let (folder, letters) = value.split_once(';').unwrap_or((value, ""));
                    self.open_folder(Path::new(folder));
                    // Its index made now, rather than in the background, for the picture.
                    if let Some(project) = &mut self.project
                        && let Some(root) = project.root.clone()
                    {
                        project.files = crate::workspace::index_with(&root, &project.explorer.hidden, &project.eff().index_options()).0;
                        // And the files edited lately found now too.
                        let times = crate::explorer_kept::outside_times(&root, &project.files, project.explorer.recent_days);
                        self.panels.project_panel.explorer.set_outside_times(&root, times);
                    }
                    if !letters.is_empty() {
                        self.open_go_to_file();
                        if let Some(state) = &mut self.go_to_file {
                            state.query = letters.to_string();
                        }
                    }
                }
                "search" => {
                    // The Search panel on some words ("find;replace"), searched now for the picture.
                    let (find, replace) = value.split_once(';').unwrap_or((value, ""));
                    self.open_search_panel();
                    self.search.query = find.to_string();
                    self.search.replace = replace.to_string();
                    self.search.show_replace = !replace.is_empty();
                    self.search_panel_now();
                }
                "line" => {
                    // To a line (and a column, as line;column), shown a few rows down so what's above it has scrolled past.
                    let (line, column) = value.split_once(';').unwrap_or((value, "0"));
                    let line = line.parse::<usize>().unwrap_or(1).saturating_sub(1);
                    let doc = self.doc();
                    doc.text.move_caret(line, column.parse().unwrap_or(0), false);
                    doc.text_state.top = line.saturating_sub(4);
                    self.outline_tick();
                    let began = std::time::Instant::now();
                    while self.outline.symbols.is_empty() && began.elapsed() < Duration::from_secs(5) {
                        std::thread::sleep(Duration::from_millis(20));
                        self.outline_tick();
                    }
                }
                "debug" => {
                    // The file in front debugged with a Python ("python;line" of a breakpoint), until it stops.
                    let (python, line) = value.split_once(';').unwrap_or((value, "1"));
                    let line = line.parse::<usize>().unwrap_or(1).saturating_sub(1);
                    let path = self.docs[self.active].path.clone().unwrap_or_default();
                    self.doc().text.code.get_or_insert_with(Box::default).breakpoints.insert(line);
                    let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
                    self.launch_debugger(crate::dap::Config { kind: crate::dap::Kind::Debugpy, adapter: format!("{python} -m debugpy.adapter"), program: path.display().to_string(), arguments: String::new(), folder, emulator: None, env: Vec::new(), stop_on_entry: false });
                    let began = std::time::Instant::now();
                    while began.elapsed() < Duration::from_secs(20) && self.debugging.as_ref().is_some_and(|d| d.parts.is_empty()) {
                        self.debug_poll();
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
                "debug-setup" => {
                    // Ctrl+F5 on the file in front: for a C program not yet built, the Debug box that asks how.
                    highlight::load_now();
                    self.doc().update_syntax();
                    self.start_debugging();
                }
                "menu" => {
                    // A menu laid open, by its place along the bar ("5" is Tools); "5+" opens its sub-menus out too.
                    let (index, out) = value.strip_suffix('+').map_or((value, false), |v| (v, true));
                    self.shown_menu = index.parse().ok().map(|i| (i, out));
                }
                // Menus opened as a click opens them, by the names on them, a sub-menu after its menu ("File>New as>Amiga").
                // File, Open recent and the start page opening on their Files tab ("files") rather than Projects.
                "recent-tab" => {
                    self.recent_lists.opens_on = if value == "files" { recent::Tab::Files } else { recent::Tab::Projects };
                }
                "open-menu" => {
                    let names: Vec<String> = value.split('>').map(str::to_string).collect();
                    if let Some(ctx) = self.waker.as_ref() {
                        ctx.data_mut(|d| d.insert_temp(crate::fit::opened_menus_key(), names));
                    }
                }
                // Any command, by its name in the menus ("Find in files", or with its menu, "Search › Find in files").
                "command" => {
                    let wanted = value.trim_end_matches('\u{2026}');
                    let found = menus::every_command().into_iter().find(|found| {
                        let path = found.path.trim_end_matches('\u{2026}');
                        path.eq_ignore_ascii_case(wanted) || path.rsplit(" \u{203A} ").next().is_some_and(|name| name.eq_ignore_ascii_case(wanted))
                    });
                    if let Some(found) = found {
                        self.pending_command = Some(found.command);
                    }
                }
                // The window made another size, to see how it looks there ("1024x480").
                "window-size" => {
                    let (w, h) = value.split_once('x').unwrap_or((value, value));
                    if let (Ok(w), Ok(h), Some(ctx)) = (w.parse::<f32>(), h.parse::<f32>(), self.waker.as_ref()) {
                        ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(w, h)));
                    }
                }
                // The ribbon on a tab, by its name ("Edit"), or its own for the file in front ("kind").
                "ribbon-tab" => {
                    self.view.ribbon = true;
                    let tabs = self.ribbon_tabs();
                    self.ribbon.tab = tabs.iter().copied().find(|tab| tab.name().eq_ignore_ascii_case(value) || value == "kind" && matches!(tab, ribbon_bar::Tab::Kind(_))).unwrap_or_default();
                    self.ribbon.popped = self.view.ribbon_folded;
                }
                // Alt's letters on the ribbon: on the tabs, or on the buttons of the tab chosen.
                "ribbon-tips" => {
                    self.ribbon.tips = if value == "buttons" { ribbon_bar::Tips::Buttons } else { ribbon_bar::Tips::Tabs };
                    self.ribbon.popped = self.view.ribbon_folded && value == "buttons";
                }
                "new-image" => self.open_new_image_box(),
                // The Workbench icon window, for the icon in front.
                "amiga-icon" => self.pending_command = Some(Command::AmigaIcon),
                "amiga-project" => {
                    // The New Amiga C project box, with a template chosen by its place in the list and a folder.
                    self.open_amiga_project_box();
                    if let Dialog::AmigaProject(state) = &mut self.dialog {
                        state.show_for_screenshot(value);
                    }
                }
                "catalog-check" => self.check_catalogs(),
                // The Disassemble box for the file in front, as Tools, Disassemble opens it.
                "disassemble" => self.open_disassemble_box(),
                // An entry chosen in the image browser in front, without opening it.
                "browser-select" => {
                    if let Some(browser) = self.docs[self.active].browser.as_mut() {
                        browser.select(value.parse().unwrap_or(0));
                    }
                }
                "php" => {
                    // A PHP tool's preview: tags, headers, arrays, upgrade or utf8.
                    let action = match value {
                        "tags" => php_host::PhpAction::LongTags,
                        "headers" => php_host::PhpAction::HeadersSent,
                        "upgrade" => php_host::PhpAction::Upgrade,
                        "utf8" => php_host::PhpAction::Utf8,
                        _ => php_host::PhpAction::ShortArrays,
                    };
                    self.php_command(action);
                }
                "regex-tester" => {
                    // The tester, with a pattern typed.
                    self.open_regex_tester(crate::regex_tester::Tab::Pattern);
                    if let Some(tester) = &mut self.regex_tester {
                        tester.pattern = value.to_string();
                    }
                }
                "code-picture" => {
                    highlight::load_now();
                    self.doc().update_syntax();
                    self.open_code_picture();
                }
                "binary-tools" => {
                    // The binary tools on a tab: checksums, patches or entropy.
                    let tab = match value {
                        "patches" => binary_tools_host::Tab::Patches,
                        "entropy" => binary_tools_host::Tab::Entropy,
                        _ => binary_tools_host::Tab::Checksums,
                    };
                    self.open_binary_tools(tab);
                }
                "go-to-file" => {
                    // Go to file, with some letters typed.
                    self.open_go_to_file();
                    if let Some(state) = &mut self.go_to_file {
                        state.query = value.to_string();
                    }
                }
                "serial" => {
                    // The serial terminal, with lines that came in (";hex" or ";screen" at the end for those views;
                    // \n a line's end and \e an escape in the text).
                    let (text, view) = value.rsplit_once(';').filter(|(_, v)| matches!(*v, "hex" | "screen" | "lines")).unwrap_or((value, ""));
                    self.serial_prefs.view = match view {
                        "hex" => crate::serial_link::View::Hex,
                        "screen" => crate::serial_link::View::Screen,
                        _ => crate::serial_link::View::Lines,
                    };
                    self.open_serial_terminal();
                    if let Some(window) = &mut self.serial {
                        window.show_sample(text.replace("\\n", "\r\n").replace("\\e", "\u{1b}").as_bytes());
                    }
                }
                "eprom" => {
                    // The EPROM programmer, run with the minipro given (a stand-in, with no programmer here), and
                    // ";CHIP" after it for a chip chosen.
                    let (program, chip) = value.split_once(';').unwrap_or((value, ""));
                    self.open_eprom();
                    if let Some(window) = &mut self.eprom {
                        if !chip.is_empty() {
                            window.choose_chip(&mut self.eprom_prefs, chip);
                        }
                        if !program.is_empty() {
                            window.use_program(PathBuf::from(program));
                        }
                        window.settle_now(&self.eprom_prefs);
                    }
                }
                // A notebook in front: "run-all", "select-N" (a cell by its place, from 0), "edit-N" (typing in it),
                // "text" (as JSON) or "kind-markdown" and the like.
                "notebook" => self.notebook_screenshot_step(value),
                // The Python or R console: "python;code" or "r;code" sent to it, waiting for its answer.
                "console" => self.console_screenshot_step(value),
                // A table in the data viewer: "sheet-N", "sort-N", "filter-WORDS" or "cell-ROW-COLUMN".
                "data" => self.data_step(value),
                // The Database panel: "sqlite=PATH" (that file, connected and opened out), "box" (adding a
                // connection), "run" (the statement at the cursor) or "panel".
                "database" => self.database_screenshot_step(value),
                // The Tests panel, with the project's tests listed; "run" runs them too.
                "tests" => self.tests_screenshot_step(value),
                // The Containers panel, for the compose file in front.
                "containers" => self.panels.containers = true,
                // The Query the rows box, with the query typed after the step's colon.
                "query-rows" => {
                    self.open_query_rows();
                    if let Dialog::QueryRows(state) = &mut self.dialog
                        && !value.is_empty()
                    {
                        state.query = value.to_string();
                    }
                }
                // Tabs down the side ("left" or "right"), and the toolbar ("on", or "box" for its buttons' box).
                "side-tabs" => {
                    self.side_tabs = side_tabs::SideTabs { on: true, right: value == "right" };
                    self.show_file_tabs = true;
                }
                "toolbar" => {
                    self.toolbar.on = true;
                    if value == "box" {
                        self.open_toolbar_box();
                    }
                }
                "colour-as" => {
                    // The Colour as box, grouped a way ("kind", "machine" or "all"), with letters typed after a ;.
                    let (view, letters) = value.split_once(';').unwrap_or((value, ""));
                    self.language_view = language_picker::View::from_setting(Some(view));
                    highlight::load_now();
                    self.open_language_picker();
                    if let Some(picker) = self.language_picker.as_mut() {
                        picker.query = letters.to_string();
                    }
                }
                // The file in front given a colour ("rose", or "none"), or a theme of its own; and a tab brought to
                // the front, by its place along the bar from 0.
                "file-colour" => {
                    if let Some(path) = self.docs[self.active].path.clone() {
                        self.set_file_tint(&path, crate::file_colours::Tint::from_id(value));
                    }
                }
                "file-theme" => self.set_file_theme(self.active, Some(value.to_string()).filter(|v| !v.is_empty())),
                // A rule colouring the open project's files ("admin/=rose").
                "colour-rule" => {
                    if let (Some(project), Some((pattern, tint))) = (self.project.as_mut(), value.split_once('=')) {
                        project.colours.rules.extend(crate::file_colours::Tint::from_id(tint).map(|tint| crate::file_colours::Rule::new(pattern, tint)));
                    }
                }
                "tab" => {
                    if let Ok(index) = value.parse::<usize>()
                        && index < self.docs.len()
                    {
                        self.active = index;
                    }
                }
                "beside" => {
                    // Another file opened beside the one in front.
                    let front = self.docs[self.active].id;
                    if self.open_path(Path::new(value)) {
                        self.show_beside(front, false);
                    }
                }
                "boot-disk" => {
                    // The Amiga boot disk tool with a plan.
                    let mut bd = crate::amiga_bootdisk_ui::BootDisk::default();
                    if let Ok(text) = std::fs::read_to_string(value)
                        && let Ok(plan) = crate::amiga_bootdisk::Plan::from_text(&text, Path::new(value).parent())
                    {
                        bd.plan = plan;
                        bd.path = Some(PathBuf::from(value));
                        bd.selected = Some(0);
                    }
                    self.boot_disk = Some(Box::new(bd));
                }
                "ripper" => {
                    // The ripper looking through a file (";graphics" after it for its graphics, at an offset).
                    let mut parts = value.split(';');
                    let path = parts.next().unwrap_or_default();
                    let mut ripper = crate::ripper_ui::Ripper::default();
                    if let Ok(bytes) = std::fs::read(path) {
                        ripper.look_at(Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), bytes);
                    }
                    if let Some(layout) = parts.next() {
                        ripper.tab = crate::ripper_ui::Tab::Graphics;
                        ripper.view.layout = crate::ripper::Layout::ALL.into_iter().find(|l| format!("{l:?}") == layout).unwrap_or(crate::ripper::Layout::Characters);
                        ripper.view.offset = parts.next().and_then(|o| usize::from_str_radix(o.trim_start_matches('$'), 16).ok()).unwrap_or(0);
                        if let Some(across) = parts.next().and_then(|a| a.parse().ok()) {
                            ripper.view.across = across;
                        }
                    }
                    self.ripper = Some(Box::new(ripper));
                }
                "retro-converter" => {
                    // A picture in the 8-bit converter, in a mode by its name ("Cpc0", "Layer2"...).
                    let (path, mode) = value.split_once(';').unwrap_or((value, "Spectrum"));
                    let mut converter = crate::retro_converter::Converter::default();
                    if let Some(m) = crate::retro_convert::Machine::ALL.iter().flat_map(|m| m.modes()).find(|m| format!("{m:?}").eq_ignore_ascii_case(mode)) {
                        converter.settings.mode = *m;
                    }
                    if let Ok(bytes) = std::fs::read(path)
                        && let Ok(image) = crate::quantise::read_picture(path, &bytes)
                    {
                        converter.set_picture(path, image);
                    }
                    self.retro_pictures = Some(Box::new(converter));
                }
                "screen-designer" => {
                    // A picture brought in, for a picture.
                    let mut designer = crate::screen_designer::Designer::default();
                    if let Ok(bytes) = std::fs::read(value)
                        && let Ok(image) = crate::quantise::read_picture(value, &bytes)
                    {
                        designer.screen = crate::screen_designer::Screen::from_image(&image);
                    }
                    self.screen_designer = Some(Box::new(designer));
                }
                "tile-map" => {
                    // A tile map or CharPad file, or "example" for a small C64 level; ";select" or ";place" after it
                    // shows a block selected, or the tile being made of characters.
                    let (file, how) = value.split_once(';').unwrap_or((value, ""));
                    let mut editor = crate::tile_map_editor::Editor::new(crate::tile_map::example());
                    if file != "example"
                        && let Ok(bytes) = std::fs::read(file)
                        && let Err(problem) = tile_map_host::load_tile_map(&mut editor, Path::new(file), &bytes)
                    {
                        eprintln!("{problem}");
                    }
                    editor.choose_tile(1);
                    match how {
                        "select" => {
                            editor.tool = crate::tile_map_editor::Tool::Select;
                            editor.selection = Some((3, 5, 6, 2));
                        }
                        "place" => editor.tile_tool = crate::tile_map_editor::TileTool::Place,
                        _ => {}
                    }
                    self.tile_map = Some(Box::new(editor));
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
                // A cartridge, or source that builds one, run on the built-in 2600 for a number of frames.
                "run-2600" => {
                    highlight::load_now();
                    self.doc().update_syntax();
                    self.assembly_tick();
                    self.run_on_target(run_host::Target::BuiltIn2600);
                    self.run_frames_for_screenshot(value.parse().unwrap_or(60));
                }
                // The same, stopped at a line's breakpoint.
                "debug-2600" => {
                    highlight::load_now();
                    self.doc().resolved_syntax = None;
                    self.doc().update_syntax();
                    self.assembly_tick();
                    let line: usize = value.parse().unwrap_or(1);
                    if let Some(code) = &mut self.doc().text.code {
                        code.breakpoints.insert(line.saturating_sub(1));
                    }
                    self.run_on_target(run_host::Target::BuiltIn2600);
                    self.run_until_stopped_for_screenshot();
                }
                "run-frames" => self.run_frames_for_screenshot(value.parse().unwrap_or(50)),
                // The controls window for a built-in machine (2600, vectrex, spectrum, zx80, zx81 or ace).
                "controls" => {
                    if let Some(pad) = crate::control_hints::Pad::ALL.into_iter().find(|p| p.id() == value) {
                        self.control_editor = Some(crate::control_hints::Editor::new(pad));
                    }
                }
                // File, Make its tape sound, on the file in front.
                "tape-sound" => self.tape_sound(),
                // A Vectrex cartridge, or source that builds one, run on the built-in Vectrex for a number of frames;
                // nobios/frames runs it without the BIOS even when one has been chosen.
                "run-vectrex" => {
                    highlight::load_now();
                    self.doc().update_syntax();
                    self.assembly_tick();
                    let (how, frames) = value.split_once('/').unwrap_or(("", value));
                    self.run_on_target(run_host::Target::BuiltInVectrex);
                    if how == "nobios" {
                        self.vectrex_without_bios();
                    }
                    self.run_frames_for_screenshot(frames.parse().unwrap_or(50));
                }
                // The file in front run on its built-in teaching machine (the Manchester Baby, EDSAC or the
                // PDP-8): as fast as it goes for a number of frames (fast/frames), or at its own speed (own/frames),
                // or until a line's breakpoint (line/N).
                "run-teaching" => {
                    highlight::load_now();
                    self.doc().update_syntax();
                    self.assembly_tick();
                    let (how, number) = value.split_once('/').unwrap_or(("fast", value));
                    let number: usize = number.parse().unwrap_or(100);
                    if how == "line"
                        && let Some(code) = &mut self.doc().text.code
                    {
                        code.breakpoints.insert(number.saturating_sub(1));
                    }
                    self.run_on_machine();
                    let frames = if how == "line" { 10_000 } else { number };
                    self.run_teaching_for_screenshot(frames, how != "own");
                }
                "run-micro" => {
                    // The file in front run on the built-in ZX80, ZX81 or Jupiter Ace, as ZX80 or ZX81 BASIC or Ace
                    // Forth if it's said to be (zx80/frames, zx81/frames, ace/frames), and left running for a number
                    // of frames.
                    let (kind, frames) = value.split_once('/').unwrap_or(("", value));
                    let stem = self.doc().name();
                    match kind {
                        "zx80" => self.doc().listing_of = Some((basic::Machine::Zx80, stem)),
                        "zx81" => self.doc().listing_of = Some((basic::Machine::Zx81, stem)),
                        "ace" => self.doc().listing_of = Some((basic::Machine::JupiterAce, stem)),
                        _ => {}
                    }
                    self.assembly_tick();
                    if let Ok(Some(run)) = self.runnable() {
                        let target = match run.machine {
                            crate::running::Machine::Zx80 => run_host::Target::Zx80,
                            crate::running::Machine::Zx81 => run_host::Target::Zx81,
                            _ => run_host::Target::Ace,
                        };
                        self.run_on(target, run);
                    }
                    self.run_frames_for_screenshot(frames.parse().unwrap_or(100));
                }
                "terminal" => {
                    // A command typed in the terminal, and a moment for it to answer.
                    if self.terminal.is_none() {
                        self.toggle_terminal();
                    }
                    self.terminal_hidden = false;
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
                    let this = self.docs[self.active].id;
                    self.open_file_compare_sides(compare_host::FileSide::Open(this), compare_host::FileSide::Path(PathBuf::from(value)));
                }
                "compare-folders" => {
                    if let Some((left, right)) = value.split_once('|') {
                        self.open_folder_compare(left, right);
                    }
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
                // F9 on the file in front (for Boriel's ZX Basic with no zxbc, the box offering to install it),
                // and the Compilers box.
                "f9" => self.run_on_machine(),
                "compilers" => self.open_compilers_box(),
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
                "call-graph" | "instructions" | "variables" => self.diagram_screenshot_step(step, ""),
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
                "extensions" => self.extensions_screenshot_step("extensions", ""),
                "themes-window" => self.open_theme_box(theme_host::ThemePage::Window),
                "themes-rules" => self.open_theme_box(theme_host::ThemePage::Rules),
                "basic-listing" => self.open_basic_listing(),
                "graph-mode" => self.toggle_graph_mode(),
                "new-kinds" => self.open_new_kinds_box(),
                "c64-raster" | "c64-irqs" => self.c64_screenshot_step(step),
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
                "compilers-box" => self.open_compilers_box(),
                "compile" => {
                    // F9 on the file opened, waiting for the compiler to finish, for the Output panel and marks.
                    highlight::load_now();
                    self.doc().resolved_syntax = None;
                    self.doc().update_syntax();
                    self.assembly_tick();
                    self.run_on_machine();
                    let began = std::time::Instant::now();
                    let ctx = egui::Context::default();
                    while began.elapsed() < Duration::from_secs(60) && self.running.compiled.busy() {
                        self.compile_tick(&ctx);
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    self.dialog = Dialog::None;
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
                // The Dragon DOS disk window with a binary and a BASIC program on its disk, the first being renamed.
                "dragon-disk" | "dragon-disk-rename" => {
                    self.open_dragon_disk();
                    if let Some(window) = &mut self.dragon_disk {
                        let _ = window.disk.add("GAME", "BIN", &crate::dragon_dos::binary(0x3000, 0x3000, &[0x39; 300]));
                        let _ = window.disk.add("LOADER", "BAS", &[0x55, 0x01, 0x1E, 0x01, 0, 4, 0x8B, 0x8D, 0xAA, 0, 0, 0, 0]);
                        if step == "dragon-disk-rename" {
                            window.renaming = Some((0, "MYGAME.BIN".into()));
                        }
                    }
                }
                "devices" | "devices-xroar" | "send" => {
                    use crate::deploy::{Device, Place};
                    self.sends.devices = vec![
                        Device { name: "Gotek".into(), place: Place::Card { label: "GOTEK".into(), folder: "GAMES/WIP".into() }, preset: None, eject: true },
                        Device { name: "Spectrum Next".into(), place: Place::NextSync { folder: "/home/me".into() }, preset: None, eject: false },
                        Device { name: "Ultimate 64".into(), place: Place::Ultimate { host: "192.168.1.64".into() }, preset: None, eject: false },
                        Device { name: "MiSTer".into(), place: Place::Sftp { address: "sftp://root@mister/media/fat/games/C64".into(), key_file: String::new() }, preset: None, eject: false },
                        Device { name: "VICE".into(), place: Place::Command { line: "x64sc -autostart {file}".into() }, preset: Some("commodore".into()), eject: false },
                    ];
                    if step == "devices-xroar" {
                        self.sends.devices.push(Device { name: "XRoar".into(), place: Place::Emulator { emulator: "xroar".into(), program: "/usr/bin/xroar".into(), options: String::new() }, preset: None, eject: false });
                    }
                    if step != "send" {
                        self.open_devices_box();
                        if let Dialog::Devices(state) = &mut self.dialog {
                            state.selected = if step == "devices" { 1 } else { 5 };
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
                "remote-folder" => {
                    self.open_remote_folder_box();
                    if let Dialog::Remote(state) = &mut self.dialog {
                        state.address = "ftpes://ann@nas.example.org/home/ann/website".into();
                        state.folder = Some("/home/ann/website".into());
                        state.status = "4 item(s)".into();
                        let entry = |name: &str, folder: bool, size: u64| crate::remote::Entry { name: name.into(), folder, size, modified: 0 };
                        state.entries = vec![entry("css", true, 0), entry("images", true, 0), entry("index.html", false, 2_310), entry("about.html", false, 1_804)];
                    }
                }
                "remote-sync" => self.remote_sync_screenshot(),
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
                    let this = doc.id;
                    self.open_file_compare_sides(compare_host::FileSide::Open(this), compare_host::FileSide::Lines { name: crate::i18n::tr("the other file").to_string(), lines: other });
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
            if !(self.prefs.reload_quietly && !modified) {
                let question = crate::i18n::tf!("{} has been changed by another application.\n\nDo you want to reload it?\n", path.display());
                if !ask_yes_no("File Changed", &question) {
                    continue;
                }
                if modified && !ask_yes_no("File Changed", "Reload file and lose changes?") {
                    continue;
                }
            }
            let doc = &self.docs[index];
            let at_end = !doc.hex_mode && doc.text.caret_line + 1 >= doc.text.line_count();
            let (line, col) = (doc.text.caret_line, doc.text.caret_col);
            match self.reopened(index, &path) {
                Ok(mut fresh) => {
                    fresh.preview_view = self.docs[index].preview_view;
                    // Following a log: at the end before, at the end now. Otherwise as near the same place as there is.
                    let text = &mut fresh.text;
                    let last = text.line_count().saturating_sub(1);
                    if at_end {
                        text.move_caret(last, usize::MAX, false);
                    } else {
                        text.move_caret(line.min(last), col, false);
                    }
                    text.reveal = true;
                    self.docs[index] = fresh;
                    self.pending_focus = true;
                }
                Err(e) => warn(&crate::i18n::tf!("Couldn't open {}.\n\n{e}", path.display())),
            }
        }
    }

    fn handle_window_events(&mut self, ctx: &Context) {
        // A script is working on the file in front, which has to stay the file in front until it's finished.
        if self.script.is_none() {
            let focused_again = ctx.input(|i| i.raw.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true))));
            let due = self.prefs.watch_often && self.last_watch.is_none_or(|last| last.elapsed() >= WATCH_EVERY);
            if self.prefs.watch_files && (focused_again || due) && matches!(self.dialog, Dialog::None) {
                self.last_watch = Some(std::time::Instant::now());
                self.check_for_changed_files();
            }
            if self.prefs.watch_files && self.prefs.watch_often {
                ctx.request_repaint_after(WATCH_EVERY);
            }
            let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
            // Files dropped on an image that can be changed are put on it; anywhere else they open.
            if !self.drop_on_image(&dropped) {
                for path in dropped {
                    // A folder dropped on the window is opened as a project, as File, Open folder opens one.
                    if path.is_dir() {
                        self.open_folder(&path);
                    } else {
                        self.open_path(&path);
                    }
                }
            }
        }

        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            self.stop_script();
            if self.settle_unsaved_for_quit() {
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
    /// A picture shows the window as it would be with nobody at it: wherever the real mouse happens to be is ignored.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if self.screenshot.is_some() {
            raw_input.events.retain(|event| !matches!(event, egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } | egui::Event::MouseWheel { .. }));
            raw_input.events.push(egui::Event::PointerGone);
        }
    }

    /// If drawing a frame ever fails, the very latest edits are written out before anything else happens.
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        recovery::IN_FRAME.store(true, Ordering::SeqCst);
        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.draw_frame(ui)));
        recovery::IN_FRAME.store(false, Ordering::SeqCst);
        open_links(ui.ctx());
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
        self.php_stop_serving();
        self.stop_split();
        self.stop_assistant();
        if self.allow_close {
            self.close_project();
        }
        if self.allow_close {
            self.finish_recovery();
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
        // The window at the size chosen; the text's own size is kept apart (see `text_view_options`).
        if (ctx.zoom_factor() - self.window_scale).abs() > 0.001 {
            ctx.set_zoom_factor(self.window_scale);
        }
        NO_FILE.set(self.no_file());
        self.follow_extensions(&ctx);
        self.apply_themes(&ctx);
        self.poll_opticos_check();
        self.poll_update_check(&ctx);
        self.music_housekeeping();
        self.control_keys_window(&ctx);
        self.read_editor_config();
        let mut edit = self.prefs.edit;
        if let Some(spaces) = self.editor_config.as_ref().and_then(|(_, c)| c.spaces).or_else(|| self.project.as_ref().and_then(|p| p.prefs.spaces)) {
            edit.spaces_for_tab = spaces;
        }
        text_buffer::set_edit_prefs(edit);
        text_buffer::set_fold_closer(self.view.fold_closer);
        // Time spent: keys, typing, clicks and scrolling in a file count; the pointer passing over doesn't.
        if self.view.track_time && !self.no_file() && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. } | egui::Event::Text(_) | egui::Event::PointerButton { pressed: true, .. } | egui::Event::MouseWheel { .. }))) {
            let doc = &self.docs[self.active];
            let folder = self.project.as_ref().map(|p| p.folder());
            let project = self.project.as_ref().map_or_else(|| crate::i18n::tr("No project").to_string(), |p| p.name.clone());
            let file = doc.path.as_deref().map_or_else(|| doc.name(), |p| crate::time_spent::file_label(p, folder.as_deref()));
            self.time_spent.activity(&project, &file, &doc.language_name(), std::time::Instant::now());
        }
        // In HTML and XML, closing tags go in by themselves and a tag's partner follows its name.
        if !self.no_file() {
            let language = self.docs[self.active].language_name();
            let markup = markup_of(&language);
            let doc = self.doc();
            doc.text.markup = markup;
            // JSX files are told by their names, as the grammar they're coloured with is JavaScript's.
            let jsx_file = language == "TypeScriptReact" || doc.path.as_deref().and_then(Path::extension).is_some_and(|e| e.eq_ignore_ascii_case("jsx") || e.eq_ignore_ascii_case("tsx"));
            doc.text.jsx = jsx_file;
            doc.text.emmet = if jsx_file { Some(crate::emmet::Syntax::Jsx) } else { crate::emmet::Syntax::of_language(&language) };
            if jsx_file {
                doc.text.markup = Some(false);
            }
            doc.text.hide_values = crate::env_file::is_env_file(&doc.path_or_name());
            doc.text.markdown = matches!(language.as_str(), "Markdown" | "MultiMarkdown");
            doc.text.rules = crate::vsix::rules_for(&language);
            // Markdown's usual problems, marked as a language server's are, when it has none of its own.
            if doc.text.markdown && !doc.hex_mode && !doc.text.is_disk_backed() && doc.text.markdown_checked != Some(doc.text.version()) && doc.text.intel.as_ref().is_none_or(|i| !i.lsp) {
                doc.text.markdown_checked = Some(doc.text.version());
                let lines = doc.text.all_lines();
                let found = crate::markdown_tools::problems(&lines);
                doc.text.intel.get_or_insert_with(Box::default).diagnostics = found;
            }
            doc.text_state.folder = doc.path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
            if !doc.hex_mode {
                doc.text.follow_tags();
            }
        }
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
        self.debug_keys(&ctx);
        // Alt's letters for the ribbon take their keys before anything else.
        self.ribbon_keys(&ctx);
        self.keyboard_routes(&ctx);
        self.shortcuts(ui);

        let mut chosen = None;
        let ribbon_open = self.view.ribbon && !self.view.ribbon_folded;
        Panel::top("menu-bar").show_separator_line(!ribbon_open).frame(egui::Frame::new().fill(theme::ribbon()).inner_margin(egui::Margin { left: 10, right: 10, top: 5, bottom: if ribbon_open { 0 } else { 5 } })).show(ui, |ui| {
            chosen = self.top_bar(ui);
        });
        // The ribbon's buttons, under its tabs, or over the text while it's folded and a tab's been clicked.
        if let Some(command) = self.ribbon_body(ui) {
            chosen = Some(command);
        }
        if let Some(command) = self.toolbar_bar(ui) {
            chosen = Some(command);
        }
        if self.show_status {
            let (mut action, mut toggle_lock) = (None, false);
            let status = Panel::bottom("status").frame(egui::Frame::new().fill(theme::ribbon()).inner_margin(egui::Margin { left: 12, right: 12, top: 4, bottom: 4 })).show(ui, |ui| (action, toggle_lock) = self.status_bar(ui));
            // A right-click where nothing else takes it chooses what the status bar shows.
            let mut layout_changed = false;
            // Clicked, but not a stop for Tab: the whole bar taking the keyboard would keep Tab from going on past it.
            crate::fit::context_menu(&status.response.interact(egui::Sense::CLICK), |ui| layout_changed = self.status_layout.menu(ui));
            if layout_changed {
                self.save_settings_if_changed();
            }
            if ui.ctx().data_mut(|d| d.remove_temp::<bool>(egui::Id::unique(language_picker::OPEN_ASKED))).unwrap_or(false) {
                self.open_language_picker();
            }
            self.python_status_asked(ui.ctx());
            if ui.ctx().data_mut(|d| d.remove_temp::<bool>(egui::Id::unique(run_host::STOP_ASKED))).unwrap_or(false) {
                self.stop_all_sessions();
            }
            self.database_status_picked(ui.ctx());
            if ui.ctx().data_mut(|d| d.remove_temp::<bool>(egui::Id::unique(remote_project_host::STATUS_CLICKED))).unwrap_or(false) {
                self.remote_project_status_clicked();
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
        self.atari2600_panel(ui);
        self.vectrex_panel(ui);
        self.micro_panel(ui);
        self.teaching_panel(ui);
        self.remote_debug_panel(ui);
        self.st_profile_panel(ui);
        let full_preview = self.docs[self.active].preview_view != settings::PreviewView::Text && self.preview_panel(ui);
        self.full_preview_shown = full_preview;
        if self.side_tabs_showing() {
            self.side_tabs_panel(ui);
        } else if self.show_file_tabs && !self.no_file() && !self.side_tabs.on {
            Panel::top("tabs").show_separator_line(false).frame(egui::Frame::new().fill(theme::window()).inner_margin(egui::Margin { left: 8, right: 8, top: 6, bottom: 0 })).show(ui, |ui| {
                self.tab_strip(ui);
            });
        }
        self.narrow_bar(ui);
        if !self.no_file() {
            self.check_for_table();
            self.table_offer_bar(ui);
            self.git_offer_bar(ui);
            self.lsp_offer_bar(ui);
            self.vscode_offer_bar(ui);
            self.env_bar(ui);
            self.conflict_bar(ui);
        }
        if self.terminal.is_some() && !self.terminal_hidden {
            self.terminal_panel(ui);
        }
        if self.debugging.is_some() {
            self.debug_panel(ui);
        }
        self.compose_logs_panel(ui);
        if self.panels.tests {
            self.tests_explorer(ui);
        } else if self.test_panel.is_some() {
            self.tests_panel(ui);
        }
        if self.show_results {
            self.results_panel(ui);
        }
        let from_menu = chosen.is_some();
        let pending = self.pending_command.take();
        if let Some(command) = chosen.or(pending) {
            // Chosen from a menu or the command box: now and then, the keys that do it.
            if from_menu || pending.is_some_and(|_| self.dialog_was_palette) {
                self.note_keys(command);
            }
            self.run(command, ui);
        }
        self.dialog_was_palette = false;
        self.show_key_note(ui);
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
        let view = self.text_view_options();
        let input_on = self.script.is_none() && matches!(self.dialog, Dialog::None) && self.pending.is_none() && !self.docs[self.active].is_busy();
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::editor_back())).show(ui, |ui| {
            context_command = self.central(ui, full_preview, view, input_on);
        });
        if let Some(ContextCommand::Explain(word)) = &context_command {
            self.open_autodocs(Some(word.clone()));
        } else if let Some(ContextCommand::DebugScript(name)) = &context_command {
            if let Some(folder) = self.docs[self.active].path.as_deref().and_then(Path::parent).map(Path::to_path_buf) {
                self.debug_npm_script(&folder, name);
            }
        } else if let Some(ContextCommand::AddProjectWord(word)) = &context_command {
            self.add_project_word(word);
        } else if let Some(command) = context_command {
            self.run(
                match command {
                    ContextCommand::Cut => Command::Cut,
                    ContextCommand::Copy => Command::Copy,
                    ContextCommand::Paste => Command::Paste,
                    ContextCommand::SelectAll => Command::SelectAll,
                    ContextCommand::Tag(colour) => Command::Tag(colour),
                    ContextCommand::Untag => Command::Untag,
                    ContextCommand::Assist => Command::Assist(assistant::Task::Fix),
                    ContextCommand::Explain(_) | ContextCommand::DebugScript(_) | ContextCommand::AddProjectWord(_) => Command::AutoDocs,
                    ContextCommand::RunCell(line) => Command::PyR(python_host::PyAction::RunCellAt(line)),
                    ContextCommand::SendRequest(number) => Command::SendRequestNumber(number),
                    ContextCommand::RunCellsAbove(line) => Command::PyR(python_host::PyAction::RunAboveAt(line)),
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
        self.focus_into_dialog(&ctx);
        self.draw_cycle(&ctx);
        self.draw_assistant_note(&ctx);
        self.pixel_editor_window(&ctx);
        self.tile_map_window(&ctx);
        self.screen_designer_window(&ctx);
        self.amiga_hw_window(&ctx);
        self.bitplane_window(&ctx);
        self.st_converter_window(&ctx);
        self.c64_converter_window(&ctx);
        self.retro_converter_window(&ctx);
        self.next_painter_window(&ctx);
        self.atari2600_graphics_window(&ctx);
        self.cart_view_window(&ctx);
        self.autodocs_window(&ctx);
        self.sound_effects_window(&ctx);
        self.ripper_window(&ctx);
        self.boot_disk_window(&ctx);
        self.project_index_tick(&ctx);
        self.outline_tick();
        self.debug_tick(&ctx);
        self.debug_setup_window(&ctx);
        self.vscode_launch_window(&ctx);
        self.go_to_file_window(&ctx);
        self.remote_sync_window(&ctx);
        self.quick_fix_window(&ctx);
        self.peek_window(&ctx);
        self.dragon_disk_window(&ctx);
        self.binary_tools_window(&ctx);
        self.regex_tester_window(&ctx);
        self.code_picture_window(&ctx);
        self.flashing_window(&ctx);
        self.serial_window(&ctx);
        self.eprom_window(&ctx);
        self.formatters_window(&ctx);
        self.time_spent_window(&ctx);
        self.php_window(&ctx);
        self.language_picker_window(&ctx);
        self.demo_tools_windows(&ctx);
        self.st_tools_windows(&ctx);
        self.c64_tools_windows(&ctx);
        self.c64_hw_windows(&ctx);
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
    The ribbon\n\
    The switch at the top right, beside Find a command, puts a ribbon in place of the menus, as Word and UltraEdit have: tabs of large and small buttons with pictures. View, Ribbon and Settings, Appearance switch it too.\n\
    Its buttons are the menus' own commands, greyed and ticked just as the menus are. Resting the pointer on one shows its keys and where it is in the menus. The File button holds the File, Window and Help menus.\n\
    A tab for the kind of file in front comes and goes with it: Assembly, BASIC, Picture, Hex, Table, Markdown or Document.\n\
    Ctrl+F1, or a double-click on a tab, folds it to its tabs; a click on a tab then shows its buttons until one's chosen.\n\
    Pressing and letting go of Alt puts a letter on each tab; a tab's letter puts letters on its buttons, and a button's letter does it. Escape goes back.\n\n\
    The start page\n\
    Once every tab is closed, the window shows the start page: new files, recent files, folders and remote files, and the guide. Settings, Appearance, Show the start page turns it off.\n\n\
    Without a mouse\n\
    F10 goes to the menus: the arrows move through them and Enter opens one. Ctrl+F6 takes the keyboard out of the text to the rest of the window, and Tab and Shift+Tab move on from there. A box that opens has the keyboard: Tab moves through it and Escape closes it.\n\n\
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
        // A comparison's own text is nothing; its sides are its own files.
        if !doc.is_modified() || doc.compare.is_some() {
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
            if doc.scanning.is_none() && doc.compare.is_none() && doc.is_modified() {
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
        Err(e) => doc.hex.notice = Some(crate::i18n::tf!("The file was saved, but couldn't be opened again: {e}")),
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
fn autosave_not_now(doc: &Document, skip_errors: bool) -> Option<&'static str> {
    if doc.path.as_deref().is_some_and(|path| file_stamp(path) != doc.disk_stamp) {
        return Some("another program has changed the file");
    }
    if skip_errors && doc.text.intel.as_ref().is_some_and(|intel| intel.lsp_problems.iter().any(|problem| problem.get("severity").and_then(serde_json::Value::as_u64) == Some(1))) {
        return Some("the language server reports errors in it");
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
        // Sent on if it's in a remote project's folder.
        None => doc.upload_wanted = true,
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
            OfferItem { selected: false, changed_on_disk: false, title: crate::i18n::tf!("A damaged copy ({name})"), detail: file.display().to_string(), note: crate::i18n::tf!("It can't be recovered. {problem} It stays in place until you discard it."), content: None, file }
        }
        Ok((meta, body)) => {
            let when = panels::format_time(Ok(std::time::UNIX_EPOCH + Duration::from_secs(meta.taken)));
            let size = match &body {
                recovery::Body::Hex(bytes) => format!("{} bytes", bytes.len()),
                recovery::Body::HexOnDisk { pieces, .. } => format!("{} bytes", pieces.iter().map(|p| p.len).sum::<usize>()),
                recovery::Body::Text(image) => format!("{} edited line(s)", image.pieces.iter().map(|piece| if let crate::line_store::ImagePiece::Memory(lines) = piece { lines.len() } else { 0 }).sum::<usize>()),
            };
            let place = meta.path.as_ref().map_or_else(|| crate::i18n::tr("Not saved yet").to_string(), |path| path.display().to_string());
            let (note, selected) = compare_with_file(&meta, &body);
            OfferItem { selected, changed_on_disk: changed_on_disk(&meta), title: meta.name.clone(), detail: format!("{place}\nKept {when}, {size}"), note, content: Some((meta, body)), file }
        }
    }
}

/// Whether a file that was there when the copy was made is there now, but not as it was.
fn changed_on_disk(meta: &recovery::Meta) -> bool {
    let Some(path) = &meta.path else { return false };
    let now = file_stamp(path);
    meta.file_stamp.is_some() && now.is_some() && now != meta.file_stamp
}

/// Whether the file on disk has moved on since, or already holds these edits.
fn compare_with_file(meta: &recovery::Meta, body: &recovery::Body) -> (String, bool) {
    let Some(path) = &meta.path else {
        return (String::new(), true);
    };
    let now = file_stamp(path);
    if now.is_none() {
        return (crate::i18n::tr("The file is no longer there. Saving will create it again.").to_string(), true);
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
        return (crate::i18n::tr("The file on disk already holds exactly this.").to_string(), false);
    }
    if meta.file_stamp.is_some() && now != meta.file_stamp {
        return (crate::i18n::tr("The file on disk has changed since these edits. Saving them would replace that version.").to_string(), true);
    }
    (String::new(), true)
}

/// The file in the recovery folder that says the last quit left unsaved files to be brought back.
const HOT_EXIT_MARKER: &str = "hot-exit";

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
    crate::fit::menu_button(ui, crate::i18n::tr("Favourites"), |ui| {
        ui.set_min_width(220.0);
        if ui.add_enabled(!find.find.is_empty(), Button::new(crate::i18n::tr("Add this search")).frame_when_inactive(false)).clicked() {
            note_search(&mut find.favourites, &find.find.clone());
        }
        if !find.favourites.is_empty() {
            ui.separator();
        }
        let mut remove = None;
        for (index, text) in find.favourites.iter().enumerate() {
            ui.horizontal(|ui| {
                if crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Remove from favourites").on_hover_text(crate::i18n::tr("Remove from favourites")).clicked() {
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
    crate::fit::menu_button(ui, crate::i18n::tr("History"), |ui| {
        ui.set_min_width(220.0);
        if find.history.is_empty() {
            ui.label(RichText::new(crate::i18n::tr("Earlier searches will be listed here.")).color(theme::text_dim()));
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

/// Whether a language is markup whose tags are paired: Some(true) for HTML, whose empty elements have no closing
/// tag, Some(false) for XML and its kin.
fn markup_of(language: &str) -> Option<bool> {
    match language {
        _ if language.starts_with("HTML") || matches!(language, "PHP" | "ASP" | "Vue Component" | "Svelte") => Some(true),
        "XML" | "SVG" | "XSL" | "XSLT" | "AmigaGuide" | "Ant" | "Maven POM" => Some(false),
        "TypeScriptReact" => Some(false),
        _ => None,
    }
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
    crate::i18n::tf!("Search string '{pattern}' not found.")
}

/// How a language's comments are marked, for Reindent, which only suits languages that go by brackets.
fn reindent_marks(language: &str) -> Result<reformat::Marks<'static>, String> {
    let no_marks = reformat::Marks::default();
    match language {
        "No highlighting" | "JSON" => return Ok(no_marks),
        "HTML" | "XML" | "SVG" => return Err(crate::i18n::tr("Reindent goes by brackets, and this goes by tags. Use Format, Lay out code, HTML or XML.").to_string()),
        _ => {}
    }
    match tools::comment_marks(language) {
        Some(marks) if marks.line == Some("//") || language == "CSS" => Ok(reformat::Marks { line: marks.line, block: marks.block, preprocessor: matches!(language, "C" | "C++" | "C#" | "Objective-C" | "Objective-C++") }),
        _ => Err(crate::i18n::tf!("Reindent goes by brackets, which doesn't suit {language}. Its indents could mean something, or have nothing to go by.")),
    }
}

/// What's said when a backup or restore finishes.
fn project_summary(result: &Result<project::Report, String>, restoring: bool) -> String {
    const MOST_LISTED: usize = 12;
    let report = match result {
        Ok(report) => report,
        Err(problem) => return if restoring { crate::i18n::tf!("Nothing was restored, because a backup of how things are now couldn't be made first.\n\n{problem}") } else { problem.clone() },
    };
    let files = |count: usize| if count == 1 { "1 file".to_string() } else { format!("{count} files") };
    let mut lines = vec![if restoring { crate::i18n::tf!("Put back {}.", files(report.done)) } else { crate::i18n::tf!("Copied {} to {}.", files(report.done), report.folder.display()) }];
    if restoring {
        lines.push(crate::i18n::tr("How they were before is kept in a backup ending in before-restore.").to_string());
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
        ui.label(crate::i18n::tr("Delimiter:"));
        for (label, value) in columns::DELIMITERS {
            ui.radio_value(delimiter, value, crate::i18n::tr(label));
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
    // A press picks its pane before either is drawn, so that the right one follows the cursor; or, with focus
    // following the mouse, the pointer being over it, while no button's held.
    let follow = view.focus_follows_mouse && ui.input(|i| !i.pointer.any_down());
    if input_on && let Some(pointer) = ui.input(|i| if i.pointer.primary_pressed() || follow { i.pointer.hover_pos() } else { None }) {
        let was = split.active;
        if a.contains(pointer) {
            split.active = 0;
        } else if b.contains(pointer) {
            split.active = 1;
        }
        // Moved into by the pointer alone, the pane takes the keyboard as a click would give it.
        if follow && split.active != was {
            ui.memory_mut(|m| m.request_focus(views::pane_id(split.active)));
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
        lines.push(crate::i18n::tf!("{} changed on disk.", plural(report.files_changed, "1 file", "files")));
    }
    if report.backups > 0 {
        lines.push(crate::i18n::tf!("{} kept beside them, ending in .bak.", plural(report.backups, "1 copy of how it was", "copies of how they were")));
    }
    if report.open_files_changed > 0 {
        lines.push(crate::i18n::tf!("{} changed in {} window, not saved yet.", plural(report.open_files_changed, "1 open file", "open files"), if report.open_files_changed == 1 { "its" } else { "their" }));
    }
    const MOST_LISTED: usize = 12;
    if !report.skipped.is_empty() {
        lines.push(String::new());
        lines.push(crate::i18n::tf!("{} left as it was:", plural(report.skipped.len(), "1 file", "files")));
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
        save = ui.button(crate::i18n::tr("Save as PNG\u{2026}")).on_hover_text(crate::i18n::tr("Save the picture as a PNG, pixel for pixel, with its colours as saved")).clicked();
        if picture.cycling.is_some() {
            let (label, hint) = if cycling.is_some() { ("\u{23F8} Stop cycling", "Stop the colours, and show the picture as it was saved") } else { ("\u{25B6} Cycle colours", "Play the picture's colour cycling, as Deluxe Paint did") };
            if ui.button(crate::i18n::tr(label)).on_hover_text(crate::i18n::tr(hint)).clicked() {
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
    ui.horizontal(|ui| (ui.button(crate::i18n::tr("OK")).clicked(), ui.button(crate::i18n::tr("Cancel")).clicked())).inner
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
    // A system box waits for a click, which a test can't give (and on Windows it would take the person's focus).
    if cfg!(test) {
        eprintln!("{message}");
        return;
    }
    crate::sys_dialogs::message_dialog().set_title(crate::i18n::tr("Limoniter")).set_level(MessageLevel::Warning).set_description(message).set_buttons(MessageButtons::Ok).show();
}

/// The kinds of file the Open box offers to show, all files first. A Mac's Open box has no list of kinds, so
/// there it shows everything.
const OPEN_FILTERS: &[(&str, &[&str])] = &[
    ("Text files", &["txt", "text", "md", "markdown", "log", "ini", "cfg", "conf", "nfo", "diz", "me", "1st", "doc", "asc"]),
    ("CSV and TSV data", &["csv", "tsv", "tab", "psv"]),
    ("Source code", &["c", "h", "cpp", "hpp", "cs", "rs", "py", "js", "ts", "java", "go", "html", "htm", "css", "json", "xml", "yaml", "yml", "toml", "sql", "sh", "bat", "cmd", "ps1", "asm", "s", "a65", "z80", "pas", "cob", "cbl", "for", "f", "rexx", "jcl", "lua", "pl", "rb", "php"]),
    ("BASIC programs", &["prg", "p00", "bas", "bbc", "tap"]),
    ("ANSI art, PETSCII and teletext", &["ans", "ice", "diz", "nfo", "xb", "pcb", "avt", "seq", "ep1", "tti", "ttix"]),
    ("Pictures", &["iff", "ilbm", "lbm", "bbm", "pic", "scr", "brush", "pcd", "koa", "kla", "art", "ocp", "dd", "pi1", "pi2", "pi3", "pc1", "pc2", "pc3", "neo", "pntg", "pnt", "mac", "ff9", "spr", "pcx", "pcc", "img", "ximg", "hgr", "gr8", "gr9", "gr7", "mic", "bmp", "sl2", "nxi", "slr"]),
    ("Disk, tape and archive images", &crate::media::IMAGE_ENDINGS),
];

fn with_open_filters(dialog: rfd::FileDialog) -> rfd::FileDialog {
    if cfg!(target_os = "macos") {
        return dialog;
    }
    OPEN_FILTERS.iter().fold(dialog.add_filter(crate::i18n::tr("All files"), &["*"]), |dialog, (name, endings)| dialog.add_filter(*name, endings))
}

fn ask_yes_no(title: &str, question: &str) -> bool {
    let answer = crate::sys_dialogs::message_dialog().set_title(title).set_level(MessageLevel::Warning).set_description(question).set_buttons(MessageButtons::YesNo).show();
    answer == MessageDialogResult::Yes
}

/// The font chosen for the window's menus, panels and boxes, if one is: read by `install_fonts`.
static WINDOW_FONT: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Fonts for the window's menus and labels, which needn't be the same width.
const WINDOW_FONT_CHOICES: [(&str, &str); 12] = [
    ("Segoe UI", "segoeui.ttf"),
    ("Arial", "arial.ttf"),
    ("Verdana", "verdana.ttf"),
    ("Tahoma", "tahoma.ttf"),
    ("Calibri", "calibri.ttf"),
    ("Ubuntu", "Ubuntu-R.ttf"),
    ("DejaVu Sans", "DejaVuSans.ttf"),
    ("Noto Sans", "NotoSans-Regular.ttf"),
    ("Liberation Sans", "LiberationSans-Regular.ttf"),
    ("Cantarell", "Cantarell-Regular.otf"),
    ("Open Sans", "OpenSans-Regular.ttf"),
    ("Inter", "Inter-Regular.ttf"),
];

fn installed_window_fonts() -> Vec<(&'static str, PathBuf)> {
    WINDOW_FONT_CHOICES.iter().filter_map(|(name, file)| font_folders().into_iter().find_map(|folder| find_font(&folder, file, 3)).map(|path| (*name, path))).collect()
}

/// The window's size, as a share of the usual, from smallest to largest.
const WINDOW_SCALES: std::ops::RangeInclusive<f32> = 0.8..=2.0;

/// Uses Consolas for the editor where Windows has it; egui's own font stays as the fallback.
fn install_fonts(ctx: &Context, chosen: Option<&Path>) {
    let mut fonts = egui::FontDefinitions::default();
    // The window's own font, if one's chosen, ahead of every other for the menus and labels.
    if let Some(path) = WINDOW_FONT.lock().ok().and_then(|f| f.clone())
        && let Ok(bytes) = std::fs::read(&path)
    {
        fonts.font_data.insert("window-font".into(), Arc::new(egui::FontData::from_owned(bytes)));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "window-font".into());
    }
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
#[path = "access_host.rs"]
mod access_host;

#[path = "addon_host.rs"]
mod addon_host;
#[path = "vsix_host.rs"]
mod vsix_host;

#[path = "remote_host.rs"]
mod remote_host;
#[path = "remote_project_host.rs"]
mod remote_project_host;
#[path = "project_import_host.rs"]
mod project_import_host;
#[path = "web_host.rs"]
mod web_host;
#[path = "clambers_host.rs"]
mod clambers_host;
#[path = "opticos_host.rs"]
mod opticos_host;
#[path = "update_host.rs"]
mod update_host;
#[path = "ndk_host.rs"]
mod ndk_host;
#[path = "amiga_project_host.rs"]
mod amiga_project_host;
#[path = "autodocs_host.rs"]
mod autodocs_host;
#[path = "st_host.rs"]
mod st_host;
#[path = "amiga_tools_host.rs"]
mod amiga_tools_host;
#[path = "amiga_icon_host.rs"]
mod amiga_icon_host;
#[path = "amiga_vamos_host.rs"]
mod amiga_vamos_host;
#[path = "music_host.rs"]
mod music_host;
#[path = "st_tools_host.rs"]
mod st_tools_host;
#[path = "c64_music_host.rs"]
mod c64_music_host;

#[path = "deploy_host.rs"]
mod deploy_host;

#[path = "run_host.rs"]
mod run_host;
#[path = "micro_host.rs"]
mod micro_host;
#[path = "teaching_host.rs"]
mod teaching_host;

#[path = "asm_host.rs"]
pub(crate) mod asm_host;

#[path = "atari2600_run_host.rs"]
mod atari2600_run_host;
#[path = "control_hints_host.rs"]
mod control_hints_host;
#[path = "vectrex_run_host.rs"]
mod vectrex_run_host;
#[path = "compile_host.rs"]
mod compile_host;

#[path = "zx_host.rs"]
mod zx_host;

#[path = "new_kinds.rs"]
mod new_kinds;
#[path = "templates_host.rs"]
mod templates_host;
#[cfg(test)]
#[path = "templates_host_tests.rs"]
mod templates_host_tests;
#[path = "ribbon_bar.rs"]
mod ribbon_bar;
#[path = "start_page.rs"]
mod start_page;
#[path = "recent.rs"]
mod recent;

#[path = "intel_host.rs"]
mod intel_host;

#[path = "diagrams_host.rs"]
mod diagrams_host;

#[path = "disasm_host.rs"]
mod disasm_host;

#[path = "pixel_host.rs"]
mod pixel_host;
#[path = "tile_map_host.rs"]
mod tile_map_host;

#[path = "amiga_hw_host.rs"]
mod amiga_hw_host;
#[path = "bitplane_host.rs"]
mod bitplane_host;
#[path = "st_converter_host.rs"]
mod st_converter_host;
#[path = "c64_converter_host.rs"]
mod c64_converter_host;
#[path = "retro_converter_host.rs"]
mod retro_converter_host;
#[path = "next_painter_host.rs"]
mod next_painter_host;
#[path = "atari2600_painter_host.rs"]
mod atari2600_painter_host;
#[path = "fantasy_carts_host.rs"]
mod fantasy_carts_host;
#[path = "sfx_designer_host.rs"]
mod sfx_designer_host;
#[path = "ripper_host.rs"]
mod ripper_host;
#[path = "amiga_bootdisk_host.rs"]
mod amiga_bootdisk_host;
#[path = "workspace_host.rs"]
mod workspace_host;
#[path = "project_settings_host.rs"]
mod project_settings_host;
#[path = "vscode_host.rs"]
mod vscode_host;
#[path = "binary_tools_host.rs"]
mod binary_tools_host;
#[path = "serial_host.rs"]
mod serial_host;
#[path = "eprom_host.rs"]
mod eprom_host;
#[path = "search_panel.rs"]
mod search_panel;
#[path = "outline.rs"]
mod outline;
#[path = "dap_host.rs"]
mod dap_host;
#[path = "dragon_disk_host.rs"]
mod dragon_disk_host;
#[path = "language_picker.rs"]
mod language_picker;
#[path = "demo_tools_host.rs"]
mod demo_tools_host;
#[path = "st_hw_host.rs"]
mod st_hw_host;
#[path = "c64_tools_host.rs"]
mod c64_tools_host;
#[path = "c64_hw_host.rs"]
mod c64_hw_host;

#[path = "lsp_host.rs"]
mod lsp_host;

#[path = "tasks_host.rs"]
mod tasks_host;
#[path = "compose_host.rs"]
mod compose_host;

#[path = "tests_host.rs"]
mod tests_host;
#[path = "side_tabs_host.rs"]
mod side_tabs_host;
#[path = "toolbar_host.rs"]
mod toolbar_host;

#[path = "outdated_host.rs"]
mod outdated_host;
#[path = "http_host.rs"]
mod http_host;

#[path = "php_host.rs"]
mod php_host;

#[path = "node_host.rs"]
mod node_host;

#[path = "live_host.rs"]
mod live_host;

#[path = "git_host.rs"]
mod git_host;

#[path = "compare_host.rs"]
pub(crate) mod compare_host;

#[path = "git_wizard.rs"]
mod git_wizard;

#[path = "merge_host.rs"]
mod merge_host;
pub use merge_host::{ToolLaunch, USAGE, parse_tool_args, tool_exit_code, unknown_option};

/// What's said of an option Limoniter doesn't know.
pub fn i18n_unknown_option(option: &str) -> String {
    crate::i18n::tf!("{option} isn't an option Limoniter knows.")
}

#[cfg(test)]
#[path = "git_host_tests.rs"]
mod git_host_tests;

#[cfg(test)]
#[path = "run_host_tests.rs"]
mod run_host_tests;

#[path = "table_view.rs"]
mod table_view;
#[path = "data_table_host.rs"]
mod data_table_host;
#[path = "csv_query_host.rs"]
mod csv_query_host;
#[path = "data_view_host.rs"]
mod data_view_host;
#[cfg(test)]
#[path = "data_view_host_tests.rs"]
mod data_view_host_tests;
#[path = "database_host.rs"]
mod database_host;

#[path = "notebook_host.rs"]
mod notebook_host;
#[path = "python_host.rs"]
mod python_host;
#[cfg(test)]
#[path = "python_host_tests.rs"]
mod python_host_tests;

#[cfg(test)]
#[path = "notebook_host_tests.rs"]
mod notebook_host_tests;

#[path = "theme_host.rs"]
mod theme_host;

#[path = "file_colours_host.rs"]
mod file_colours_host;

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

/// How many times each command's keys are noted, at most, in a session.
const KEY_NOTES_EACH: u8 = 3;
/// How long a note of the keys stays.
const KEY_NOTE_FOR: Duration = Duration::from_secs(5);

/// The keys for a command, as shown, if the menus give it any.
fn keys_for_command(command: Command) -> Option<(String, String)> {
    menus::every_command().into_iter().find(|found| found.command == command && !found.shortcut.is_empty()).map(|found| (found.path, menus::shown_shortcut(found.shortcut)))
}

impl Limoniter {
    fn note_keys(&mut self, command: Command) {
        if !self.view.learn_keys {
            return;
        }
        let Some((path, keys)) = keys_for_command(command) else { return };
        let given = self.key_notes_given.entry(path.clone()).or_insert(0);
        if *given >= KEY_NOTES_EACH {
            return;
        }
        *given += 1;
        let name = path.rsplit(" \u{203A} ").next().unwrap_or(&path).to_string();
        self.key_note = Some((crate::i18n::tf!("{keys} does {name} without the menu."), std::time::Instant::now()));
    }

    fn show_key_note(&mut self, ui: &mut Ui) {
        let Some((words, when)) = &self.key_note else { return };
        let age = when.elapsed();
        if age >= KEY_NOTE_FOR {
            self.key_note = None;
            return;
        }
        let fade = 1.0 - ((age.as_secs_f32() - (KEY_NOTE_FOR.as_secs_f32() - 1.0)).max(0.0));
        let screen = ui.ctx().content_rect();
        egui::Area::new(egui::Id::unique("key-note")).order(egui::Order::Tooltip).interactable(false).pivot(egui::Align2::RIGHT_BOTTOM).fixed_pos(egui::pos2(screen.max.x - 16.0, screen.max.y - 40.0)).show(ui.ctx(), |ui| {
            ui.set_opacity(fade);
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(RichText::new(format!("\u{2328}  {words}")).size(13.0));
            });
        });
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }
}

#[cfg(test)]
#[path = "file_tools_tests.rs"]
mod file_tools_tests;

#[cfg(test)]
#[path = "fit_tests.rs"]
mod fit_tests;

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
        let (orphans, found) = recovery::find_orphans_once_let_go(&root);
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

    /// How To Code reads like a book in one tab: a link turns the page in place, Back (Alt+Left) goes back through
    /// the pages read, and Forward returns, without a tab left behind for each page.
    #[test]
    fn a_link_in_how_to_code_turns_the_page_in_the_same_tab() {
        let (mut app, _ctx) = app_with("");
        app.open_how_to_code();
        let readme = app.docs[app.active].path.clone().expect("the guide opened");
        assert!(is_guide_page(&readme), "{readme:?}");
        let tabs = app.docs.len();
        let folder = readme.parent().unwrap().to_path_buf();
        let (one, two) = (folder.join("01-introduction.md"), folder.join("02-working-with-limoniter.md"));
        let mut back = std::mem::take(&mut app.docs[app.active].guide_back);
        back.push(readme.clone());
        app.turn_guide_page(&one, back, Vec::new());
        let mut back = std::mem::take(&mut app.docs[app.active].guide_back);
        back.push(one.clone());
        app.turn_guide_page(&two, back, Vec::new());
        assert_eq!(app.docs.len(), tabs, "still one tab for the guide");
        assert_eq!(app.docs[app.active].path.as_deref(), Some(two.as_path()));
        app.go_through_history(true);
        assert_eq!(app.docs[app.active].path.as_deref(), Some(one.as_path()));
        app.go_through_history(true);
        assert_eq!(app.docs[app.active].path.as_deref(), Some(readme.as_path()));
        app.go_through_history(false);
        assert_eq!(app.docs[app.active].path.as_deref(), Some(one.as_path()), "and forward again");
        assert_eq!(app.docs.len(), tabs);
    }

    pub(super) fn app_with(settings: &str) -> (Limoniter, Context) {
        let mut app = Limoniter::build(settings.to_string(), None, Some(PathBuf::from("never-written.png")));
        app.new_document();
        (app, Context::default())
    }

    pub(super) fn input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), events, ..Default::default() }
    }

    pub(super) fn draw(app: &mut Limoniter, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut output = ctx.run_ui(input(events), |ui| app.draw_frame(ui));
        output.textures_delta.clear();
        output
    }

    pub(super) fn command(app: &mut Limoniter, ctx: &Context, command: Command) -> egui::FullOutput {
        let mut output = ctx.run_ui(input(Vec::new()), |ui| app.run(command, ui));
        output.textures_delta.clear();
        output
    }

    fn key(key: Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: if modifiers.ctrl { modifiers | egui::Modifiers::COMMAND } else { modifiers } }
    }

    const EVERY_PANEL: &str = "file_properties=true\njson_manager=true\nxml_manager=true\nascii_table=true\nencoding_preview=true\nclipboard_history=false\nfile_explorer=true\nopen_files=true\nbookmarks=true\nfile_lists=true\nmacros_panel=true\nscripts_panel=true\nhistory_panel=true\n";

    #[test]
    fn close_the_others_keeps_the_file_in_front_and_close_all_leaves_none() {
        let (mut app, _ctx) = app_with("");
        app.new_document();
        app.new_document();
        app.new_document();
        let count = app.docs.len();
        assert!(count >= 3);
        app.active = 1;
        let kept = app.docs[1].id;
        app.close_all(Some(1));
        assert_eq!(app.docs.len(), 1);
        assert_eq!((app.active, app.docs[0].id), (0, kept), "the file in front stays, and is still in front");
        app.new_document();
        app.close_all(None);
        assert!(app.no_file(), "nothing is left but the empty window");
        // With nothing open, both do nothing.
        app.close_all(None);
        app.close_all(Some(0));
        assert!(app.no_file());
    }

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
    fn restored_settings_are_used_at_once_and_keep_what_panels_show() {
        let (mut app, ctx) = app_with("");
        app.panels.recent = vec![PathBuf::from("before.txt")];
        let saved = crate::settings_file::bundle("design=2\nruler=true\nstatus_bar=false\nfind_history=needle\nrecent_file=after.txt\nfunctions=true\n", "", "0", "today");
        let (settings, _) = crate::settings_file::unbundle(&saved).unwrap();
        app.apply_settings(&ctx, settings);
        assert!(app.view.ruler && !app.show_status && app.panels.functions);
        assert_eq!(app.find.history, ["needle"]);
        assert_eq!(app.panels.recent, [PathBuf::from("after.txt")]);
        draw(&mut app, &ctx, Vec::new());
    }

    #[test]
    fn a_growing_log_is_reloaded_quietly_and_followed_at_its_end() {
        let folder = std::env::temp_dir().join(format!("limoniter-watch-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let log = folder.join("server.log");
        std::fs::write(&log, "one\ntwo\n").unwrap();
        let (mut app, _ctx) = app_with("reload_quietly=true\n");
        app.open_path(&log);
        let index = app.active;
        let last = app.docs[index].text.line_count() - 1;
        app.docs[index].text.move_caret(last, 0, false);
        std::fs::write(&log, "one\ntwo\nthree\nfour\n").unwrap();
        app.docs[index].disk_stamp = None;
        app.screenshot = None;
        app.check_for_changed_files();
        let text = &app.docs[index].text;
        assert_eq!(text.line(2).as_ref(), "three", "reloaded without asking");
        assert_eq!(text.caret_line, text.line_count() - 1, "still at the end");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_selection_is_surrounded_and_stays_selected_inside() {
        let (mut app, ctx) = app_with("");
        app.doc().text.set_content(vec!["say hello there".into()]);
        app.doc().text.select_chars((0, 4), (0, 9));
        command(&mut app, &ctx, Command::Surround('(', ')'));
        assert_eq!(lines_of(&app), ["say (hello) there"]);
        assert_eq!(app.doc().text.selection_text(), "hello");
        app.surround_with_tag("b class=\"x\"").unwrap();
        assert_eq!(lines_of(&app), ["say (<b class=\"x\">hello</b>) there"]);
        assert!(app.surround_with_tag("2x").is_err());
        command(&mut app, &ctx, Command::Undo);
        assert_eq!(lines_of(&app), ["say (hello) there"], "each surround is one undo");
    }

    #[test]
    fn the_value_inspector_draws_in_text_and_in_hex() {
        let (mut app, ctx) = app_with("inspector_panel=true\n");
        assert!(app.panels.inspector);
        app.doc().text.set_content(vec!["ABCDEFGH".into()]);
        draw(&mut app, &ctx, Vec::new());
        command(&mut app, &ctx, Command::ViewHex);
        draw(&mut app, &ctx, Vec::new());
        assert!(app.docs[app.active].hex_mode);
    }

    #[test]
    fn the_structure_panel_takes_a_file_apart_and_follows_the_hex_cursor() {
        let folder = std::env::temp_dir().join(format!("limoniter-structure-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("program");
        // A small Amiga program: the hunk header, one code hunk and its end.
        let mut program = Vec::new();
        for long in [0x3F3u32, 0, 1, 0, 0, 2, 0x3E9, 2, 0x4E75_4E75, 0x4E71_4E71, 0x3F2] {
            program.extend(long.to_be_bytes());
        }
        std::fs::write(&file, &program).unwrap();
        let (mut app, ctx) = app_with("structure_panel=true\n");
        app.open_path(&file);
        command(&mut app, &ctx, Command::ViewHex);
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, Vec::new());
        let doc = &app.docs[app.active];
        let tree = app.panels.structure_panel.marks(doc.id, doc.hex.version()).expect("read with a template");
        assert_eq!(tree.error, None);
        let find = |tree: &crate::structure::Tree, path: &str| (1..tree.nodes.len()).find(|i| tree.path(*i) == path).unwrap();
        // The hex cursor moved into a field chooses it in the tree.
        app.doc().hex.move_caret(13, false);
        draw(&mut app, &ctx, Vec::new());
        let tree = app.panels.structure_panel.tree().unwrap();
        assert_eq!(app.panels.structure_panel.selected(), Some(find(tree, "first_hunk")));
        // A field chosen in the tree selects its bytes, and stays chosen though the cursor is in a field inside it.
        let block = find(tree, "hunks[0]");
        let mut action = app.panels.structure_panel.choose_node(block);
        let mut frame = ctx.run_ui(input(Vec::new()), |ui| {
            if let Some(action) = action.take() {
                app.run_panel_action(action, ui);
            }
        });
        frame.textures_delta.clear();
        draw(&mut app, &ctx, Vec::new());
        let hex = &app.docs[app.active].hex;
        assert_eq!((hex.anchor.min(hex.caret), hex.anchor.max(hex.caret)), (24, 40));
        assert_eq!(app.panels.structure_panel.selected(), Some(block));
        // The hex view names the field under a byte.
        let doc = &app.docs[app.active];
        let tree = app.panels.structure_panel.marks(doc.id, doc.hex.version()).unwrap();
        assert_eq!(tree.describe(33, &doc.hex).as_deref(), Some("hunks[0].contents[1]: 117 (0x75)"));
        // An edit is read again at once.
        app.doc().hex.move_caret(32, false);
        app.doc().hex.type_byte(20);
        draw(&mut app, &ctx, Vec::new());
        let doc = &app.docs[app.active];
        let tree = app.panels.structure_panel.marks(doc.id, doc.hex.version()).unwrap();
        assert_eq!(tree.describe(32, &doc.hex).as_deref(), Some("hunks[0].contents[0]: 20 (0x14)"));
        // A change that makes it another kind of file leaves it with no template.
        app.doc().hex.move_caret(0, false);
        app.doc().hex.type_byte(20);
        draw(&mut app, &ctx, Vec::new());
        let doc = &app.docs[app.active];
        assert!(app.panels.structure_panel.marks(doc.id, doc.hex.version()).is_none());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_old_word_processors_document_opened_by_itself_is_shown_converted() {
        let folder = std::env::temp_dir().join(format!("limoniter-wordstar-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("LETTER.WS");
        std::fs::write(&file, b"Dear Sir, this letter\x8D\nis written in\x8D\nWordStar, as they were.\r\n").unwrap();
        let plain = folder.join("notes.txt");
        std::fs::write(&plain, "Caf\u{E9} au lait, et cr\u{E8}me br\u{FB}l\u{E9}e \u{E0} la carte.\n").unwrap();
        let (mut app, ctx) = app_with("");
        app.open_path(&file);
        let doc = &app.docs[app.active];
        assert_eq!(doc.legacy, Some(crate::legacy_docs::Kind::WordStar));
        assert_eq!(Limoniter::preview_kind(doc), Some(preview::Kind::Document));
        assert!(app.can(Command::SaveDocumentAs(crate::legacy_docs::Target::Markdown)));
        let bytes = std::fs::read(&file).unwrap();
        assert!(matches!(preview::render_bytes(preview::Kind::Document, &bytes, &doc.text.format, Some(&file)), preview::FromBytes::Blocks(blocks) if !blocks.is_empty()));
        draw(&mut app, &ctx, Vec::new());
        app.open_path(&plain);
        assert_eq!(app.docs[app.active].legacy, None, "ordinary UTF-8 text isn't taken for WordStar");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_saved_layout_puts_the_panels_back_and_is_kept_in_the_settings() {
        let (mut app, ctx) = app_with("");
        app.panels.functions = true;
        app.show_results = true;
        app.dialog = Dialog::SaveLayout { name: "Debugging".into(), error: String::new() };
        draw(&mut app, &ctx, vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
        assert_eq!(app.layouts.len(), 1, "saved by pressing Enter");
        app.panels.functions = false;
        app.show_results = false;
        app.apply_layout(0);
        assert!(app.panels.functions && app.show_results);
        let settings = app.current_settings();
        assert_eq!(crate::layouts::from_settings(&settings), app.layouts);
    }

    #[test]
    fn conversions_change_the_selection_or_the_whole_file_and_undo() {
        let (mut app, ctx) = app_with("");
        app.doc().text.set_content(vec!["foobar".into()]);
        command(&mut app, &ctx, Command::Convert(conversions::Conversion::Base64Encode));
        assert_eq!(lines_of(&app), ["Zm9vYmFy"]);
        command(&mut app, &ctx, Command::Convert(conversions::Conversion::Base64Decode));
        assert_eq!(lines_of(&app), ["foobar"]);
        command(&mut app, &ctx, Command::Convert(conversions::Conversion::Base64Decode));
        assert_eq!(lines_of(&app), ["foobar"], "a problem leaves the text alone");
        command(&mut app, &ctx, Command::Undo);
        assert_eq!(lines_of(&app), ["Zm9vYmFy"]);
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
        app.dialog = Dialog::None;
        let (ours, theirs) = (app.docs[0].id, app.docs[1].id);
        app.open_file_compare_sides(compare_host::FileSide::Open(ours), compare_host::FileSide::Open(theirs));
        let Some(compare_host::CompareTab::Files(files)) = app.docs[app.active].compare.as_deref() else {
            panic!("it's showing, in a tab of its own");
        };
        assert_eq!(files.diff.difference_count(), 1);
        assert_eq!(files.diff.current, Some(1), "it starts at the first difference");
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(app.docs.len(), 3);
        app.active = 0;
        assert_eq!(lines_of(&app), ["same", "ours"], "comparing changes nothing");
    }

    #[test]
    fn compare_explains_what_it_cant_take() {
        let (mut app, ctx) = app_with("");
        let this = app.docs[0].id;
        app.open_file_compare_sides(compare_host::FileSide::Open(this), compare_host::FileSide::Open(this));
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("same file")));
        app.dialog = Dialog::None;
        app.open_file_compare_sides(compare_host::FileSide::Open(this), compare_host::FileSide::Path(PathBuf::from("/no/such/file.txt")));
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("isn't a file")));
        app.dialog = Dialog::None;
        // A compare tab can't itself be compared.
        app.open_folder_compare_sides(None, None);
        command(&mut app, &ctx, Command::Compare);
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("isn't a file")));
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
    fn the_kinds_last_chosen_in_new_as_come_first_and_are_kept() {
        let (mut app, _ctx) = app_with("");
        let at = |app: &Limoniter, name: &str| app.new_kinds.iter().position(|k| k.name == name).unwrap();
        for name in ["Z80 assembler", "Markdown", "Python", "Markdown", "C"] {
            app.new_of_kind(at(&app, name));
        }
        assert_eq!(app.recent_new_kinds, ["C", "Markdown", "Python"], "the latest first, each once, three at most");
        let (back, _ctx) = app_with("recent_new_kind=C\nrecent_new_kind=Markdown\n");
        assert_eq!(back.recent_new_kinds, ["C", "Markdown"], "kept with the settings");
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
    fn links_are_opened_by_limoniter_itself() {
        let ctx = Context::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            ui.ctx().open_url(egui::OpenUrl::new_tab("https://example.com/"));
            open_links(ui.ctx());
        });
        assert!(!output.platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(_))), "taken, not left for eframe, which can't open it");
    }

    #[test]
    fn the_about_box_opens_the_licences_to_read() {
        let (mut app, ctx) = app_with("");
        let notices = std::env::temp_dir().join(format!("limoniter-notices-{}.txt", std::process::id()));
        std::fs::write(&notices, "Third-party notices\n").unwrap();
        ctx.enable_accesskit();
        draw(&mut app, &ctx, Vec::new());
        app.dialog = Dialog::About { notices: Some(notices.clone()) };
        draw(&mut app, &ctx, Vec::new());
        let output = draw(&mut app, &ctx, Vec::new());
        let nodes = crate::access::tests::nodes(&output);
        let button = nodes.iter().map(|(_, n)| n).filter(|n| n.role() == egui::accesskit::Role::Button && n.label().is_some_and(|l| l.starts_with("Licence information"))).find_map(|n| n.bounds()).expect("a Licence information button");
        let centre = egui::pos2(((button.x0 + button.x1) / 2.0) as f32, ((button.y0 + button.y1) / 2.0) as f32);
        for events in click_at(centre) {
            draw(&mut app, &ctx, events);
        }
        assert!(matches!(app.dialog, Dialog::None), "the box closes");
        let doc = &app.docs[app.active];
        assert_eq!(doc.path.as_deref(), Some(notices.as_path()));
        assert!(doc.text.read_only, "to read, not to change");
        let _ = std::fs::remove_file(notices);
    }

    #[test]
    fn the_command_box_has_a_close_button() {
        let (mut app, ctx) = app_with("");
        ctx.enable_accesskit();
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, vec![key(Key::F1, egui::Modifiers::NONE)]);
        let output = draw(&mut app, &ctx, Vec::new());
        let nodes = crate::access::tests::nodes(&output);
        let close = nodes.iter().map(|(_, n)| n).filter(|n| n.role() == egui::accesskit::Role::Button && n.label().is_some_and(|l| l == "Close")).find_map(|n| n.bounds()).expect("a Close button");
        let centre = egui::pos2(((close.x0 + close.x1) / 2.0) as f32, ((close.y0 + close.y1) / 2.0) as f32);
        for events in click_at(centre) {
            draw(&mut app, &ctx, events);
        }
        assert!(matches!(app.dialog, Dialog::None), "the box is closed");
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
        assert!(matches!(&app.dialog, Dialog::ProjectSettings(draft) if draft.name == "p"));
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
        let (orphans, found) = recovery::find_orphans_once_let_go(root);
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

        let (orphans, found) = recovery::find_orphans_once_let_go(&root);
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
        let (orphans, found) = recovery::find_orphans_once_let_go(&root);
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
        let item = offer_item(recovery::Found { file: PathBuf::from("doc-1.rec"), outcome: Err(crate::i18n::tr("Its checksum doesn't match.").to_string()) });
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

