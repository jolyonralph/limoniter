//! The menu bar, laid out by what things are for, and a box that finds any command by its name (F1).
//!
//! Everything reachable from the menus is set out in the tables below, once. The menus draw from them, the command
//! box searches them, and a test checks that nothing is left out.

use super::*;
use crate::preview;
use crate::text_objects::{Action as TextAction, Object as TextObject};

/// A switch that's simply on or off, toggled from a menu or the command box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Flag {
    LineNumbers,
    Ruler,
    DocumentMap,
    BookmarkGutter,
    FoldLines,
    LineChange,
    Whitespace,
    LineEndings,
    SourceView,
    ActiveLine,
    ActiveColumn,
    Rainbow,
    HighlightSelected,
    FileTabs,
    StatusBar,
    OutputWindow,
    WordWrap,
    ReadOnly,
    ColumnMode,
    AlwaysOnTop,
    FullScreen,
    Spelling,
    GitBlame,
    AssistantPanel,
    Panel(PanelFlag),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PanelFlag {
    FileExplorer,
    Project,
    OpenFiles,
    FileLists,
    Functions,
    Bookmarks,
    Tags,
    Macros,
    Scripts,
    History,
    FileProperties,
    Json,
    Xml,
    Characters,
    EncodingPreview,
    Clipboard,
    Strings,
}

/// Lists that change, drawn as they are now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Live {
    WindowThemes,
    DocumentThemes,
    ThisDocumentTheme,
    Devices,
    Machines,
    RecentFiles,
    NewAs,
    RecentProjects,
    Language,
    Macros,
    Scripts,
    AddOns,
    TabSize,
    Snippets,
    Tags,
}

pub(super) enum Entry {
    Item(&'static str, Command),
    Keyed(&'static str, Command, &'static str),
    Check(&'static str, Flag),
    Sub(&'static str, &'static [Entry]),
    Live(&'static str, Live),
    Line,
}

use Entry::{Check, Item, Keyed, Line, Sub};

const fn key(keyed: bool, text: &'static str) -> &'static str {
    if keyed { text } else { "" }
}

const fn objects(action: TextAction, keyed: bool) -> [Entry; 6] {
    [
        Keyed("Quotes", Command::TextObject(action, TextObject::Quotes), key(keyed, "Ctrl+Shift+Q")),
        Keyed("Brackets", Command::TextObject(action, TextObject::Brackets), key(keyed, "Ctrl+Shift+B")),
        Keyed("Tag", Command::TextObject(action, TextObject::Tag), key(keyed, "Ctrl+Shift+T")),
        Keyed("Word", Command::TextObject(action, TextObject::Word), key(keyed, "Ctrl+Shift+W")),
        Keyed("Line", Command::TextObject(action, TextObject::Line), key(keyed, "Ctrl+Shift+L")),
        Keyed("Paragraph", Command::TextObject(action, TextObject::Paragraph), key(keyed, "Ctrl+Shift+G")),
    ]
}

const SELECT_INSIDE: [Entry; 6] = objects(TextAction::SelectInside, true);
const SELECT_AROUND: [Entry; 6] = objects(TextAction::SelectAround, false);
const REPLACE_INSIDE: [Entry; 6] = objects(TextAction::ReplaceInside, false);
const DELETE_INSIDE: [Entry; 6] = objects(TextAction::DeleteInside, false);
const DELETE_AROUND: [Entry; 6] = objects(TextAction::DeleteAround, false);

const FILE: &[Entry] = &[
    Keyed("New", Command::New, "Ctrl+N"),
    Entry::Live("New as", Live::NewAs),
    Keyed("Open\u{2026}", Command::Open, "Ctrl+O"),
    Entry::Live("Open recent", Live::RecentFiles),
    Item("Open a remote file (SFTP)\u{2026}", Command::OpenRemote),
    Line,
    Keyed("Save", Command::Save, "Ctrl+S"),
    Keyed("Save as\u{2026}", Command::SaveAs, "Ctrl+Shift+S"),
    Item("Save the selection as\u{2026}", Command::SaveSelection),
    Item("Save a copy in another encoding\u{2026}", Command::SaveCopyAs),
    Item("Save to a server (SFTP)\u{2026}", Command::SaveRemote),
    Sub("Send to device", &[Entry::Live("Devices", Live::Devices), Line, Keyed("Send again", Command::SendAgain, "Ctrl+Alt+S"), Item("Manage devices\u{2026}", Command::ManageDevices)]),
    Keyed("Run on machine", Command::RunOnMachine, "F9"),
    Sub(
        "Run on",
        &[
            Entry::Live("Machines", Live::Machines),
            Line,
            Item("Its memory", Command::MachineMemory),
            Item("Its screen as text", Command::MachineScreen),
            Item("Take its BASIC program", Command::MachineBasicOut),
            Item("Put this listing into it", Command::MachineBasicIn),
            Line,
            Item("Stop running", Command::StopRunning),
            Line,
            Item("Assemble and save", Command::AssembleAndSave),
            Item("Assemble and save as binary\u{2026}", Command::AssembleBinary),
            Item("Assemblers\u{2026}", Command::ChooseAssembler),
            Item("Devices and emulators\u{2026}", Command::ManageDevices),
        ],
    ),
    Line,
    Sub(
        "This file",
        &[
            Item("Rename\u{2026}", Command::RenameFile),
            Item(crate::platform::SHOW_IN_FILE_MANAGER, Command::ShowInExplorer),
            Item("Open in its usual program", Command::OpenWithDefault),
            Line,
            Item("Split into parts\u{2026}", Command::SplitFile),
        ],
    ),
    Item("Reopen with another encoding\u{2026}", Command::ReopenAs),
    Item("Recover unsaved edits\u{2026}", Command::RecoverEdits),
    Line,
    Keyed("Print\u{2026}", Command::Print, "Ctrl+P"),
    Line,
    Item("New window", Command::NewInstance),
];

const EDIT: &[Entry] = &[
    Keyed("Undo", Command::Undo, "Ctrl+Z"),
    Keyed("Redo", Command::Redo, "Ctrl+Y"),
    Keyed("Edit history", Command::ShowHistory, "Ctrl+Shift+H"),
    Keyed("History of this part", Command::PartHistory, "Ctrl+Alt+H"),
    Line,
    Keyed("Cut", Command::Cut, "Ctrl+X"),
    Keyed("Copy", Command::Copy, "Ctrl+C"),
    Keyed("Paste", Command::Paste, "Ctrl+V"),
    Keyed("Swap with clipboard", Command::SwapClipboard, "Ctrl+Shift+V"),
    Line,
    Keyed("Select all", Command::SelectAll, "Ctrl+A"),
    Sub(
        "Select",
        &[
            Sub("Inside", &SELECT_INSIDE),
            Sub("Around", &SELECT_AROUND),
            Line,
            Item("To the matching bracket", Command::SelectToBrace),
            Item("Every copy of the selection", Command::MakeSelections),
        ],
    ),
    Sub("Replace inside", &REPLACE_INSIDE),
    Sub("Delete", &[Sub("Inside", &DELETE_INSIDE), Sub("Around", &DELETE_AROUND)]),
    Line,
    Sub(
        "Lines",
        &[
            Item("Move up", Command::MoveLineUp),
            Item("Move down", Command::MoveLineDown),
            Keyed("Duplicate", Command::Duplicate, "Ctrl+D"),
            Item("Join", Command::Join),
            Line,
            Item("Sort A to Z", Command::SortAscending),
            Item("Sort Z to A", Command::SortDescending),
            Line,
            Item("Indent", Command::Indent),
            Item("Outdent", Command::Outdent),
            Item("Trim leading spaces", Command::TrimLeading),
            Item("Trim trailing spaces", Command::TrimTrailing),
        ],
    ),
    Sub(
        "Columns",
        &[
            Check("Column mode  (Alt+C)", Flag::ColumnMode),
            Line,
            Item("Insert or fill\u{2026}", Command::ColumnFill),
            Item("Cut columns\u{2026}", Command::ColumnCut),
            Item("Delete columns\u{2026}", Command::ColumnDelete),
            Line,
            Item("Justify left", Command::JustifyLeft),
            Item("Justify centre", Command::JustifyCenter),
            Item("Justify right", Command::JustifyRight),
            Line,
            Item("Insert numbers\u{2026}", Command::ColumnNumber),
            Item("Add up the numbers", Command::ColumnSum),
        ],
    ),
    Sub(
        "Change case",
        &[
            Keyed("UPPER CASE", Command::Upper, "Alt+U"),
            Keyed("lower case", Command::Lower, "Alt+L"),
            Item("Sentence case", Command::SentenceCase),
            Item("Title Case", Command::TitleCase),
        ],
    ),
    Sub(
        "Insert",
        &[
            Item("Date", Command::InsertDate),
            Item("Time", Command::InsertTime),
            Item("Date and time", Command::InsertDateTime),
            Item("Date, year first", Command::InsertIsoDate),
            Line,
            Item("File name", Command::InsertFileName),
            Item("File path", Command::InsertFilePath),
            Item("GUID", Command::GenerateGuid),
            Item("Page break", Command::InsertPageBreak),
            Line,
            Item("Colour\u{2026}", Command::InsertColor),
            Item("A byte\u{2026}", Command::InsertByte),
            Item("Another file\u{2026}", Command::InsertFile),
        ],
    ),
    Sub("Snippets", &[Keyed("Choose or make one\u{2026}", Command::Snippets, "Ctrl+J"), Line, Entry::Live("Insert", Live::Snippets)]),
    Sub(
        "Colour tag",
        &[
            Keyed("Red", Command::Tag(0), "Ctrl+Shift+1"),
            Keyed("Orange", Command::Tag(1), "Ctrl+Shift+2"),
            Keyed("Yellow", Command::Tag(2), "Ctrl+Shift+3"),
            Keyed("Green", Command::Tag(3), "Ctrl+Shift+4"),
            Keyed("Blue", Command::Tag(4), "Ctrl+Shift+5"),
            Keyed("Purple", Command::Tag(5), "Ctrl+Shift+6"),
            Line,
            Keyed("Remove the tag", Command::Untag, "Ctrl+Shift+0"),
        ],
    ),
    Line,
    Check("Read-only", Flag::ReadOnly),
];

const SEARCH: &[Entry] = &[
    Keyed("Find\u{2026}", Command::Find, "Ctrl+F"),
    Keyed("Replace\u{2026}", Command::Replace, "Ctrl+R"),
    Keyed("Find next", Command::FindNext, "F3"),
    Keyed("Find previous", Command::FindPrevious, "Shift+F3"),
    Keyed("Find in files\u{2026}", Command::FindInFiles, "Ctrl+Shift+F"),
    Line,
    Item("Only show lines with the selection", Command::FilterSelection),
    Line,
    Keyed("Go to line\u{2026}", Command::GoTo, "Ctrl+G"),
    Keyed("Go to definition", Command::GoToDefinition, "F12"),
    Keyed("Find all uses", Command::FindUses, "Shift+F12"),
    Keyed("Rename label\u{2026}", Command::RenameLabel, "Ctrl+F12"),
    Item("Go to the matching bracket", Command::GoToBrace),
    Keyed("Back", Command::NavBack, "Alt+Left"),
    Keyed("Forward", Command::NavForward, "Alt+Right"),
    Item("Previous change", Command::LastChange),
    Item("Next change", Command::NextChange),
    Line,
    Sub(
        "Bookmarks",
        &[
            Keyed("Set or clear", Command::ToggleBookmark, "Ctrl+F2"),
            Keyed("Next", Command::NextBookmark, "F2"),
            Keyed("Previous", Command::PreviousBookmark, "Shift+F2"),
            Line,
            Check("Bookmarks panel", Flag::Panel(PanelFlag::Bookmarks)),
        ],
    ),
    Sub("Colour tags", &[Keyed("Next", Command::NextTag, "F4"), Keyed("Previous", Command::PreviousTag, "Shift+F4"), Line, Entry::Live("Go to", Live::Tags)]),
];

const VIEW: &[Entry] = &[
    Item("Larger text", Command::ZoomIn),
    Item("Smaller text", Command::ZoomOut),
    Item("Font\u{2026}", Command::ChooseFont),
    Sub(
        "Theme",
        &[
            Sub("Window", &[Entry::Live("Window themes", Live::WindowThemes)]),
            Sub("Documents", &[Entry::Live("Document themes", Live::DocumentThemes)]),
            Sub("This document only", &[Entry::Live("This document's theme", Live::ThisDocumentTheme)]),
            Line,
            Item("Themes and rules\u{2026}", Command::Themes),
        ],
    ),
    Line,
    Check("Word wrap", Flag::WordWrap),
    Sub(
        "Show",
        &[
            Check("Line numbers", Flag::LineNumbers),
            Check("Ruler", Flag::Ruler),
            Check("Document map", Flag::DocumentMap),
            Check("Bookmark margin", Flag::BookmarkGutter),
            Check("Fold markers", Flag::FoldLines),
            Check("Changed lines", Flag::LineChange),
            Line,
            Check("Spaces and tabs", Flag::Whitespace),
            Check("Line endings", Flag::LineEndings),
            Check("Control characters as names", Flag::SourceView),
            Line,
            Check("The line with the cursor", Flag::ActiveLine),
            Check("The column with the cursor", Flag::ActiveColumn),
            Check("Rainbow brackets", Flag::Rainbow),
            Check("Every copy of the selection", Flag::HighlightSelected),
            Check("Spelling mistakes", Flag::Spelling),
        ],
    ),
    Sub(
        "Folding",
        &[Item("Fold everything", Command::CollapseAll), Item("Unfold everything", Command::ExpandAll), Line, Item("Hide the selected lines", Command::HideLines), Item("Show the selected lines", Command::ShowLines)],
    ),
    Keyed("Narrow to the selection", Command::Narrow, "Ctrl+Shift+N"),
    Item("Show the whole file", Command::Widen),
    Keyed("Table view (CSV, TSV)", Command::ToggleTable, "F6"),
    Line,
    Sub("Preview", &[Item("Off", Command::SetPreview(settings::PreviewView::Text)), Item("Beside the text", Command::SetPreview(settings::PreviewView::Split)), Item("Instead of the text", Command::SetPreview(settings::PreviewView::Full))]),
    Sub(
        "Hex",
        &[
            Item("As text", Command::ViewRegular),
            Keyed("As hex", Command::ViewHex, "Ctrl+H"),
            Item("As hex, in EBCDIC", Command::ViewEbcdic),
            Line,
            Item("Insert or delete bytes\u{2026}", Command::HexInsertDelete),
            Item("Copy the selected rows", Command::HexCopySelected),
            Line,
            Check("String explorer", Flag::Panel(PanelFlag::Strings)),
        ],
    ),
    Sub("Split", &[Item("One above the other", Command::SplitHorizontal), Item("Side by side", Command::SplitVertical), Item("Another tab on this file", Command::DuplicateWindow)]),
    Line,
    Sub(
        "Panels",
        &[
            Check("Files on this computer", Flag::Panel(PanelFlag::FileExplorer)),
            Check("Project", Flag::Panel(PanelFlag::Project)),
            Check("Open files", Flag::Panel(PanelFlag::OpenFiles)),
            Check("File lists", Flag::Panel(PanelFlag::FileLists)),
            Line,
            Check("Functions", Flag::Panel(PanelFlag::Functions)),
            Check("Bookmarks", Flag::Panel(PanelFlag::Bookmarks)),
            Check("Tags", Flag::Panel(PanelFlag::Tags)),
            Check("Edit history", Flag::Panel(PanelFlag::History)),
            Check("Macros", Flag::Panel(PanelFlag::Macros)),
            Check("Scripts", Flag::Panel(PanelFlag::Scripts)),
            Line,
            Check("File properties", Flag::Panel(PanelFlag::FileProperties)),
            Check("JSON", Flag::Panel(PanelFlag::Json)),
            Check("XML", Flag::Panel(PanelFlag::Xml)),
            Check("Characters", Flag::Panel(PanelFlag::Characters)),
            Check("Encoding preview", Flag::Panel(PanelFlag::EncodingPreview)),
            Check("String explorer", Flag::Panel(PanelFlag::Strings)),
            Check("Clipboard history", Flag::Panel(PanelFlag::Clipboard)),
            Line,
            Item("Close every panel", Command::HidePanels),
        ],
    ),
    Check("Output", Flag::OutputWindow),
    Check("File tabs", Flag::FileTabs),
    Check("Status bar", Flag::StatusBar),
    Line,
    Check("Always on top", Flag::AlwaysOnTop),
    Check("Full screen", Flag::FullScreen),
];

const FORMAT: &[Entry] = &[
    Sub(
        "Encoding",
        &[
            Item("Convert to\u{2026}", Command::ConvertTo),
            Item("Reopen with\u{2026}", Command::ReopenAs),
            Item("Save a copy in\u{2026}", Command::SaveCopyAs),
            Item("Kind of computer for new files\u{2026}", Command::ChoosePreset),
        ],
    ),
    Sub(
        "Tokenised BASIC",
        &[
            Item("Open the listing as text", Command::OpenBasicListing),
            Item("Save as a Spectrum tape (.tap)\u{2026}", Command::SaveSpectrumTape),
            Item("Save as a Spectrum Next program (.bas)\u{2026}", Command::SaveNextProgram),
            Item("Save as a Commodore program (.prg)\u{2026}", Command::SaveCommodoreProgram),
            Item("Save as a BBC BASIC program\u{2026}", Command::SaveBbcProgram),
            Line,
            Keyed("Spectrum GRAPH mode", Command::GraphMode, "Ctrl+Shift+9"),
            Item("Spectrum UDG pictures from\u{2026}", Command::UdgPictures),
        ],
    ),
    Item("Save the picture as PNG\u{2026}", Command::SavePictureAsPng),
    Line,
    Item("Reindent", Command::Reindent),
    Item("Tabs to spaces", Command::TabsToSpaces),
    Item("Spaces to tabs", Command::SpacesToTabs),
    Entry::Live("Tab size", Live::TabSize),
    Line,
    Sub(
        "Paragraph",
        &[
            Item("Reformat", Command::ParagraphReformat),
            Item("Width and margins\u{2026}", Command::ParagraphSetup),
            Line,
            Item("Align left", Command::AlignText(Align::Left)),
            Item("Centre", Command::AlignText(Align::Center)),
            Item("Align right", Command::AlignText(Align::Right)),
            Item("Justify", Command::AlignText(Align::Justify)),
            Line,
            Item("Break long lines at a column\u{2026}", Command::WrapToLineEnds),
            Item("Join each paragraph into one line", Command::LineEndsToWrap),
        ],
    ),
    Sub(
        "Lay out code",
        &[
            Item("HTML", Command::Format(formatters::Kind::Html)),
            Item("XML", Command::Format(formatters::Kind::Xml)),
            Item("JSON", Command::FormatJson),
            Item("JavaScript", Command::Format(formatters::Kind::Js)),
            Item("CSS", Command::Format(formatters::Kind::Css)),
            Item("SQL", Command::Format(formatters::Kind::Sql)),
            Item("C-like code, brace on the same line", Command::Format(formatters::Kind::CodeAttach)),
            Item("C-like code, brace on its own line", Command::Format(formatters::Kind::CodeBreak)),
            Item("EDI, one segment to a line", Command::Format(formatters::Kind::EdiLines)),
        ],
    ),
    Sub(
        "Compress",
        &[
            Item("HTML", Command::Format(formatters::Kind::HtmlCompact)),
            Item("XML", Command::Format(formatters::Kind::XmlCompact)),
            Item("JSON", Command::CompactJson),
            Item("JavaScript", Command::Format(formatters::Kind::JsCompact)),
            Item("CSS", Command::Format(formatters::Kind::CssCompact)),
            Item("EDI, onto one line", Command::Format(formatters::Kind::EdiOneLine)),
        ],
    ),
    Sub("Table", &[Item("Comma-separated to fixed columns\u{2026}", Command::CsvToFixed), Item("Fixed columns to comma-separated\u{2026}", Command::FixedToCsv)]),
    Line,
    Item("Comment out", Command::Comment),
    Item("Uncomment", Command::Uncomment),
];

const TOOLS: &[Entry] = &[
    Entry::Live("Colour as", Live::Language),
    Line,
    Item("Compare with\u{2026}", Command::Compare),
    Item("Count words", Command::WordCount),
    Item("Fingerprint (SHA-256)", Command::ComputeHash),
    Item("Convert a number", Command::ConvertNumber),
    Item("Copy the file's path", Command::CopyPath),
    Item("Disassemble\u{2026}", Command::Disassemble),
    Item("Character and sprite editor", Command::PixelEditor),
    Item("Spectrum screen designer", Command::ScreenDesigner),
    Line,
    Sub(
        "Git",
        &[
            Keyed("Source control", Command::SourceControl, "Ctrl+Shift+G"),
            Item("Compare with the last commit", Command::GitCompareHead),
            Item("History of this file\u{2026}", Command::GitHistory),
            Check("Show who changed each line", Flag::GitBlame),
            Item("Put the change at the cursor in the next commit", Command::GitStageChange),
            Item("Undo the change at the cursor", Command::GitUndoChange),
            Line,
            Item("Git wizard\u{2026}", Command::GitWizard),
        ],
    ),
    Sub("Spelling", &[Keyed("Next mistake", Command::NextMistake, "F7"), Check("Underline mistakes", Flag::Spelling)]),
    Sub(
        "Assistant",
        &[
            Check("Assistant panel", Flag::AssistantPanel),
            Line,
            Keyed("Fix\u{2026}", Command::Assist(assistant::Task::Fix), "Ctrl+Shift+A"),
            Item("Clean up\u{2026}", Command::Assist(assistant::Task::CleanUp)),
            Item("Write code\u{2026}", Command::Assist(assistant::Task::Write)),
            Item("Explain\u{2026}", Command::Assist(assistant::Task::Explain)),
            Item("Ask about it\u{2026}", Command::Assist(assistant::Task::Ask)),
        ],
    ),
    Sub("Run", &[Keyed("Run a command\u{2026}", Command::RunTool, "F5"), Item("Stop it", Command::StopTool), Line, Check("Output panel", Flag::OutputWindow)]),
    Sub(
        "Macros",
        &[
            Keyed("Record or stop", Command::RecordMacro, "Ctrl+Shift+R"),
            Keyed("Play", Command::PlayMacro, "Ctrl+Shift+P"),
            Item("Play several times\u{2026}", Command::PlayMacroTimes),
            Entry::Live("Play a saved macro", Live::Macros),
            Line,
            Item("Load\u{2026}", Command::LoadMacro),
            Check("Macros panel", Flag::Panel(PanelFlag::Macros)),
        ],
    ),
    Sub(
        "Scripts",
        &[Entry::Live("Run", Live::Scripts), Line, Item("New script\u{2026}", Command::NewScript), Item("Load\u{2026}", Command::LoadScript), Check("Scripts panel", Flag::Panel(PanelFlag::Scripts))],
    ),
    Sub("Add-ons", &[Entry::Live("Commands", Live::AddOns), Line, Item("Manage add-ons\u{2026}", Command::ManageAddOns), Item("New add-on\u{2026}", Command::NewAddOn)]),
    Sub(
        "Project",
        &[
            Item("New project\u{2026}", Command::ProjectNew),
            Item("Open project\u{2026}", Command::ProjectOpen),
            Entry::Live("Open recent", Live::RecentProjects),
            Item("Close project", Command::ProjectClose),
            Item("Name and notes\u{2026}", Command::ProjectSettings),
            Line,
            Item("Add this file", Command::ProjectAddActive),
            Item("Add files\u{2026}", Command::ProjectAddFiles),
            Item("Add a folder\u{2026}", Command::ProjectAddFolder),
            Line,
            Item("Back up", Command::ProjectBackup),
            Item("Restore from a backup\u{2026}", Command::ProjectRestore),
            Check("Project panel", Flag::Panel(PanelFlag::Project)),
        ],
    ),
    Line,
    Item("Settings\u{2026}", Command::Settings),
];

const WINDOW: &[Entry] = &[
    Keyed("Last file used", Command::UsedFileBack, "Ctrl+Tab"),
    Keyed("Least recently used file", Command::UsedFileForward, "Ctrl+Shift+Tab"),
    Item("Previous tab", Command::PreviousTab),
    Item("Next tab", Command::NextTab),
    Check("List of open files", Flag::Panel(PanelFlag::OpenFiles)),
    Item("Recently opened files", Command::ShowFileLists),
    Line,
    Item("New window", Command::NewInstance),
];

const HELP_MENU: &[Entry] = &[Keyed("Find a command\u{2026}", Command::CommandPalette, "F1"), Item("How things work", Command::Help), Item("What Limoniter can do", Command::Features), Item("Limoniter website", Command::Website), Line, Item("About Limoniter", Command::About)];

pub(super) const MENUS: [(&str, &[Entry]); 8] = [("File", FILE), ("Edit", EDIT), ("Search", SEARCH), ("View", VIEW), ("Format", FORMAT), ("Tools", TOOLS), ("Window", WINDOW), ("Help", HELP_MENU)];

/// A shortcut as the menus show it. On a Mac it's in the Mac's own signs, with Cmd for Ctrl, and the few keys a
/// Mac uses differently: Cmd+[ and Cmd+] for back and forward, and Control and Option for the Alt letters.
pub(super) fn shown_shortcut(shortcut: &str) -> String {
    if !cfg!(target_os = "macos") || shortcut.is_empty() {
        return shortcut.to_string();
    }
    mac_shortcut(shortcut)
}

/// The Mac's way of writing a shortcut: its signs in the order Control, Option, Shift, Cmd, then the key.
pub(super) fn mac_shortcut(shortcut: &str) -> String {
    match shortcut {
        "Alt+Left" => return "\u{2318}[".into(),
        "Alt+Right" => return "\u{2318}]".into(),
        "Ctrl+Tab" => return "\u{2303}Tab".into(),
        "Ctrl+Shift+Tab" => return "\u{2303}\u{21E7}Tab".into(),
        _ => {}
    }
    let parts: Vec<&str> = shortcut.split('+').collect();
    let (held, key) = parts.split_at(parts.len() - 1);
    let alt_letter = held == ["Alt"] && key[0].len() == 1;
    let has = |name: &str| held.contains(&name);
    let mut out = String::new();
    if alt_letter {
        out.push('\u{2303}');
    }
    if has("Alt") {
        out.push('\u{2325}');
    }
    if has("Shift") {
        out.push('\u{21E7}');
    }
    if has("Ctrl") {
        out.push('\u{2318}');
    }
    out += match key[0] {
        "Left" => "\u{2190}",
        "Right" => "\u{2192}",
        "Up" => "\u{2191}",
        "Down" => "\u{2193}",
        other => other,
    };
    out
}

/// Something chosen from a list that changes.
enum Pick {
    Command(Command),
    File(PathBuf),
    Project(PathBuf),
    Play(PathBuf),
    AddOn(usize),
    Device(usize),
    RunOn(Option<usize>),
    WindowTheme(String),
    DocumentTheme(String),
    ThisDocumentTheme(Option<String>),
    Language(Language),
    Snippet(usize),
    Line(usize),
}

/// One line in the command box.
pub(super) struct Found {
    pub path: String,
    pub shortcut: &'static str,
    pub command: Command,
}

#[derive(Default)]
pub(super) struct PaletteState {
    query: String,
    selected: usize,
    focus: bool,
}

impl PaletteState {
    pub(super) fn new() -> Self {
        Self { focus: true, ..Self::default() }
    }

    pub(super) fn query_for_picture(&mut self, words: &str) {
        self.query = words.to_string();
    }
}

/// Every command, with where it is in the menus, so the command box can find it by any of those words.
pub(super) fn every_command() -> Vec<Found> {
    fn walk(entries: &[Entry], path: &str, out: &mut Vec<Found>) {
        for entry in entries {
            let named = |label: &str| if path.is_empty() { label.to_string() } else { format!("{path} \u{203A} {label}") };
            match entry {
                Item(label, command) => out.push(Found { path: named(label), shortcut: "", command: *command }),
                Keyed(label, command, key) => out.push(Found { path: named(label), shortcut: key, command: *command }),
                Check(label, flag) => out.push(Found { path: named(label), shortcut: "", command: Command::Toggle(*flag) }),
                Sub(label, inner) => walk(inner, &named(label), out),
                Entry::Live(..) | Line => {}
            }
        }
    }
    let mut out = Vec::new();
    for (title, entries) in MENUS {
        walk(entries, title, &mut out);
    }
    out
}

/// Which commands match what's typed. Every word has to be there, and those whose own name starts with it come first.
pub(super) fn matching(query: &str, all: Vec<Found>) -> Vec<Found> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut found: Vec<Found> = all.into_iter().filter(|each| words.iter().all(|word| each.path.to_lowercase().contains(word.as_str()))).collect();
    if let Some(first) = words.first() {
        found.sort_by_key(|each| !each.path.rsplit('\u{203A}').next().unwrap_or("").trim().to_lowercase().starts_with(first.as_str()));
    }
    found
}

const MENU_WIDTH: f32 = 250.0;

impl Limoniter {
    fn panel_on(&self, panel: PanelFlag) -> bool {
        let p = &self.panels;
        match panel {
            PanelFlag::FileExplorer => p.file_explorer,
            PanelFlag::Project => p.project,
            PanelFlag::OpenFiles => p.open_files,
            PanelFlag::FileLists => p.file_lists,
            PanelFlag::Functions => p.functions,
            PanelFlag::Bookmarks => p.bookmarks,
            PanelFlag::Tags => p.tags,
            PanelFlag::Macros => p.macros,
            PanelFlag::Scripts => p.scripts,
            PanelFlag::History => p.history,
            PanelFlag::FileProperties => p.file_properties,
            PanelFlag::Json => p.json_manager,
            PanelFlag::Xml => p.xml_manager,
            PanelFlag::Characters => p.ascii_table,
            PanelFlag::EncodingPreview => p.encoding_preview,
            PanelFlag::Clipboard => p.clipboard_history,
            PanelFlag::Strings => p.strings,
        }
    }

    fn panel_switch(&mut self, panel: PanelFlag) -> &mut bool {
        let p = &mut self.panels;
        match panel {
            PanelFlag::FileExplorer => &mut p.file_explorer,
            PanelFlag::Project => &mut p.project,
            PanelFlag::OpenFiles => &mut p.open_files,
            PanelFlag::FileLists => &mut p.file_lists,
            PanelFlag::Functions => &mut p.functions,
            PanelFlag::Bookmarks => &mut p.bookmarks,
            PanelFlag::Tags => &mut p.tags,
            PanelFlag::Macros => &mut p.macros,
            PanelFlag::Scripts => &mut p.scripts,
            PanelFlag::History => &mut p.history,
            PanelFlag::FileProperties => &mut p.file_properties,
            PanelFlag::Json => &mut p.json_manager,
            PanelFlag::Xml => &mut p.xml_manager,
            PanelFlag::Characters => &mut p.ascii_table,
            PanelFlag::EncodingPreview => &mut p.encoding_preview,
            PanelFlag::Clipboard => &mut p.clipboard_history,
            PanelFlag::Strings => &mut p.strings,
        }
    }

    pub(super) fn flag_on(&self, flag: Flag) -> bool {
        let doc = &self.docs[self.active];
        let v = &self.view;
        match flag {
            Flag::LineNumbers => v.line_numbers,
            Flag::Ruler => v.ruler,
            Flag::DocumentMap => v.document_map,
            Flag::BookmarkGutter => v.bookmark_gutter,
            Flag::FoldLines => v.fold_lines,
            Flag::LineChange => v.line_change,
            Flag::Whitespace => v.whitespace,
            Flag::LineEndings => v.line_endings,
            Flag::SourceView => v.source_view,
            Flag::ActiveLine => v.active_line,
            Flag::ActiveColumn => v.active_col,
            Flag::Rainbow => v.rainbow_parentheses,
            Flag::HighlightSelected => v.highlight_selected,
            Flag::FileTabs => self.show_file_tabs,
            Flag::StatusBar => self.show_status,
            Flag::OutputWindow => self.show_results,
            Flag::WordWrap => v.word_wrap,
            Flag::ReadOnly => doc.text.read_only,
            Flag::ColumnMode => !doc.hex_mode && doc.text.column_mode(),
            Flag::AlwaysOnTop => self.always_on_top,
            Flag::FullScreen => self.full_screen,
            Flag::Spelling => self.prefs.spelling,
            Flag::GitBlame => self.git.blame_on,
            Flag::AssistantPanel => self.assist_panel.open,
            Flag::Panel(panel) => self.panel_on(panel),
        }
    }

    pub(super) fn toggle_flag(&mut self, flag: Flag, ui: &Ui) {
        let v = &mut self.view;
        let switch: &mut bool = match flag {
            Flag::LineNumbers => &mut v.line_numbers,
            Flag::Ruler => &mut v.ruler,
            Flag::DocumentMap => &mut v.document_map,
            Flag::BookmarkGutter => &mut v.bookmark_gutter,
            Flag::FoldLines => &mut v.fold_lines,
            Flag::LineChange => &mut v.line_change,
            Flag::Whitespace => &mut v.whitespace,
            Flag::LineEndings => &mut v.line_endings,
            Flag::SourceView => &mut v.source_view,
            Flag::ActiveLine => &mut v.active_line,
            Flag::ActiveColumn => &mut v.active_col,
            Flag::Rainbow => &mut v.rainbow_parentheses,
            Flag::HighlightSelected => &mut v.highlight_selected,
            Flag::FileTabs => &mut self.show_file_tabs,
            Flag::StatusBar => &mut self.show_status,
            Flag::OutputWindow => &mut self.show_results,
            Flag::GitBlame => {
                self.git.blame_on = !self.git.blame_on;
                return;
            }
            Flag::AssistantPanel => {
                self.assist_panel.open = !self.assist_panel.open;
                return;
            }
            Flag::Spelling => {
                self.prefs.spelling = !self.prefs.spelling;
                // Asking again after Windows said it couldn't is worth a try: a dictionary may have been added since.
                self.spell_problem = None;
                return;
            }
            // These have more to do than change a value, so they go through their own commands.
            Flag::WordWrap => return self.run(Command::ToggleWrap, ui),
            Flag::ReadOnly => return self.run(Command::ToggleReadOnly, ui),
            Flag::ColumnMode => return self.run(Command::ToggleColumn, ui),
            Flag::AlwaysOnTop => return self.run(Command::AlwaysOnTop, ui),
            Flag::FullScreen => return self.run(Command::FullScreen, ui),
            Flag::Panel(PanelFlag::Macros) => return self.run(Command::ShowMacros, ui),
            Flag::Panel(PanelFlag::Scripts) => return self.run(Command::ShowScripts, ui),
            Flag::Panel(panel) => {
                let on = self.panel_switch(panel);
                *on = !*on;
                if matches!(panel, PanelFlag::Macros | PanelFlag::Scripts) {
                    self.library.refresh();
                }
                return;
            }
        };
        *switch = !*switch;
    }

    /// Whether a command can do anything now, so that menus and the command box can say so.
    pub(super) fn can(&self, command: Command) -> bool {
        let doc = &self.docs[self.active];
        let no_file = self.no_file();
        if no_file && !command.works_without_file() {
            return false;
        }
        let hex = doc.hex_mode;
        let text = !hex && !no_file;
        let t = &doc.text;
        let selected = if hex { doc.hex.has_selection() } else { t.has_selection() || (self.prefs.edit.whole_line_copy && !t.is_block()) };
        let one_line = text && t.has_selection() && t.caret_line == t.anchor_line && !t.is_block();
        let has_project = self.project.is_some();
        match command {
            Command::Undo => if hex { doc.hex.can_undo() } else { t.can_undo() },
            Command::Redo => if hex { doc.hex.can_redo() } else { t.can_redo() },
            Command::Copy | Command::Cut => selected,
            Command::FindNext | Command::FindPrevious => !self.find.find.is_empty(),
            Command::NavBack => self.jumps.can_go_back(),
            Command::NavForward => self.jumps.can_go_forward(),
            Command::LastChange | Command::NextChange => text && t.changed_line_count() > 0,
            Command::NextBookmark | Command::PreviousBookmark => text && !t.bookmarks().is_empty(),
            Command::CopyPath | Command::RenameFile | Command::ShowInExplorer | Command::OpenWithDefault => doc.path.is_some(),
            Command::SendTo(n) if matches!(self.sends.devices.get(n).map(|d| &d.place), Some(crate::deploy::Place::Emulator { .. })) => !no_file,
            Command::SendTo(_) | Command::SendAgain => !no_file && doc.browser.is_none(),
            Command::RunOnMachine | Command::RunOnBuiltIn | Command::RunOn(_) => !no_file,
            Command::GoToDefinition | Command::FindUses | Command::RenameLabel => !no_file && self.doc_cpu(self.active).is_some(),
            Command::AssembleAndSave | Command::AssembleBinary => !no_file && self.doc_cpu(self.active).is_some(),
            Command::StopRunning => !self.running.sessions.is_empty(),
            Command::MachineMemory | Command::MachineScreen | Command::MachineBasicOut => self.reachable().is_some(),
            Command::MachineBasicIn => self.reachable().is_some() && doc.listing_of.is_some(),
            Command::GitCompareHead | Command::GitHistory | Command::GitStageChange => self.git.repo.is_some() && doc.path.is_some(),
            Command::GitUndoChange => self.git.repo.is_some() && doc.path.is_some() && !doc.text.read_only,
            Command::ToggleTable => doc.table.is_some() || (text && doc.browser.is_none()),
            Command::SwapClipboard | Command::SaveSelection => if hex { doc.hex.has_selection() } else { t.has_selection() },
            Command::SavePictureAsPng => doc.picture.is_some(),
            Command::GraphMode | Command::UdgPictures => !no_file && self.is_spectrum_basic(self.active),
            Command::OpenBasicListing => !t.is_disk_backed() && preview::kind_for_file(doc.path.as_deref(), "", &t.format) == Some(preview::Kind::Basic),
            Command::SaveSpectrumTape | Command::SaveNextProgram | Command::SaveCommodoreProgram | Command::SaveBbcProgram => text && !t.is_disk_backed(),
            Command::UsedFileBack | Command::UsedFileForward => self.docs.iter().filter(|doc| !doc.placeholder).count() > 1,
            Command::StopTool => self.tool_running(),
            Command::SplitFile => text && doc.path.is_some(),
            Command::PartHistory => text && !t.is_disk_backed() && doc.narrow.is_none(),
            Command::Untag => text && self.tag_here_to_remove(),
            Command::NextTag | Command::PreviousTag => text && !t.tags().is_empty(),
            Command::Assist(_) => text && !t.is_disk_backed(),
            Command::ColumnFill | Command::ColumnCut | Command::ColumnDelete | Command::JustifyLeft | Command::JustifyCenter | Command::JustifyRight | Command::ColumnNumber => text && t.is_block(),
            Command::MakeSelections => one_line,
            Command::FilterSelection => doc.filter.is_some() || one_line,
            Command::ExpandAll => text && t.has_folds(),
            Command::Narrow => text && doc.narrow.is_none(),
            Command::Widen => doc.narrow.is_some(),
            Command::HexInsertDelete => hex,
            Command::HexCopySelected => hex && doc.hex.has_selection(),
            Command::Comment | Command::Uncomment => text && tools::comment_marks(&doc.language_name()).is_some(),
            Command::Upper | Command::Lower | Command::SentenceCase | Command::TitleCase => text && t.has_selection(),
            Command::PreviousTab | Command::NextTab => self.all_tabs().len() > 1,
            Command::ProjectClose | Command::ProjectSettings | Command::ProjectAddFiles | Command::ProjectAddFolder | Command::ProjectBackup | Command::ProjectRestore => has_project,
            Command::ProjectAddActive => has_project && doc.path.is_some(),
            Command::PlayMacro | Command::PlayMacroTimes => self.current_macro.is_some(),
            Command::ZoomIn => self.view.font_size < 40.0,
            Command::ZoomOut => self.view.font_size > 8.0,
            Command::Toggle(Flag::ColumnMode) => text,
            Command::Toggle(Flag::ReadOnly) => !no_file,
            // Hex mode has its own versions of these, or they're about the file rather than its text.
            Command::New
            | Command::Open
            | Command::Save
            | Command::SaveAs
            | Command::CloseTab
            | Command::Paste
            | Command::SelectAll
            | Command::Find
            | Command::Replace
            | Command::FindInFiles
            | Command::GoTo
            | Command::ToggleHex
            | Command::ViewRegular
            | Command::ViewHex
            | Command::ViewEbcdic
            | Command::Settings
            | Command::Print
            | Command::ReopenAs
            | Command::SaveCopyAs
            | Command::ChoosePreset
            | Command::InsertByte
            | Command::RecoverEdits
            | Command::ChooseFont
            | Command::ShowFileLists
            | Command::AlwaysOnTop
            | Command::FullScreen
            | Command::NewInstance
            | Command::Help
            | Command::CommandPalette
            | Command::About
            | Command::ToggleWrap
            | Command::ToggleReadOnly
            | Command::TogglePreview
            | Command::SetPreview(_)
            | Command::ProjectOpen
            | Command::ProjectNew
            | Command::RecordMacro
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
            | Command::HidePanels
            | Command::InsertDate
            | Command::InsertTime
            | Command::InsertDateTime
            | Command::InsertIsoDate
            | Command::InsertFileName
            | Command::InsertFilePath
            | Command::InsertPageBreak
            | Command::InsertColor
            | Command::InsertFile
            | Command::Snippets
            | Command::RunTool
            | Command::Toggle(_) => true,
            _ => text,
        }
    }

    fn menu_item(&self, ui: &mut Ui, label: &str, shortcut: &str, command: Command, chosen: &mut Option<Command>) {
        let button = Button::new(label).shortcut_text(RichText::new(shown_shortcut(shortcut)).color(theme::text_faint())).min_size(egui::vec2(MENU_WIDTH - 16.0, 0.0));
        if ui.add_enabled(self.can(command), button).clicked() {
            *chosen = Some(command);
            ui.close();
        }
    }

    /// Draws a menu's entries. Opened out, every sub-menu is drawn in place, for a picture of everything or a test.
    fn draw_entries(&mut self, ui: &mut Ui, entries: &[Entry], opened_out: bool, chosen: &mut Option<Command>, picks: &mut Vec<Pick>) {
        for entry in entries {
            match entry {
                Item(label, command) => self.menu_item(ui, label, "", *command, chosen),
                Keyed(label, command, key) => self.menu_item(ui, label, key, *command, chosen),
                Check(label, flag) => {
                    let mut on = self.flag_on(*flag);
                    if ui.add_enabled(self.can(Command::Toggle(*flag)), egui::Checkbox::new(&mut on, *label)).clicked() {
                        *chosen = Some(Command::Toggle(*flag));
                    }
                }
                Sub(label, inner) if opened_out => {
                    ui.label(RichText::new(*label).color(theme::text_dim()));
                    ui.indent(*label, |ui| self.draw_entries(ui, inner, true, chosen, picks));
                }
                // Laid open for a picture, a sub-menu shows as it does in a menu that's been clicked: a name and an arrow.
                Sub(label, _) if self.drawing_open => {
                    ui.add(Button::new(*label).shortcut_text(RichText::new("\u{203A}").color(theme::text_faint())).min_size(egui::vec2(MENU_WIDTH - 16.0, 0.0)));
                }
                Sub(label, inner) => {
                    ui.menu_button(*label, |ui| {
                        ui.set_min_width(MENU_WIDTH);
                        self.draw_entries(ui, inner, false, chosen, picks);
                    });
                }
                Entry::Live(label, live) if opened_out || *live == Live::TabSize => self.live_entries(ui, *live, label, chosen, picks),
                Entry::Live(label, _) if self.drawing_open => {
                    ui.add(Button::new(*label).shortcut_text(RichText::new("\u{203A}").color(theme::text_faint())).min_size(egui::vec2(MENU_WIDTH - 16.0, 0.0)));
                }
                Entry::Live(label, live) => {
                    ui.menu_button(*label, |ui| {
                        ui.set_min_width(MENU_WIDTH);
                        self.live_entries(ui, *live, label, chosen, picks);
                    });
                }
                Line => {
                    ui.separator();
                }
            }
        }
    }

    fn live_entries(&mut self, ui: &mut Ui, live: Live, label: &str, chosen: &mut Option<Command>, picks: &mut Vec<Pick>) {
        let empty = |ui: &mut Ui, words: &str| {
            ui.label(RichText::new(words).color(theme::text_faint()));
        };
        let name_of = |path: &Path| path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
        match live {
            Live::RecentFiles => {
                if self.panels.recent.is_empty() {
                    empty(ui, "Files you open will be listed here.");
                }
                for path in self.panels.recent.iter().take(15) {
                    if ui.button(name_of(path)).on_hover_text(path.display().to_string()).clicked() {
                        picks.push(Pick::File(path.clone()));
                        ui.close();
                    }
                }
            }
            Live::NewAs => {
                for (index, kind) in self.new_kinds.iter().enumerate() {
                    let ending = if kind.ending.is_empty() { String::new() } else { format!(".{}", kind.ending) };
                    if ui.add(Button::new(kind.name.as_str()).shortcut_text(RichText::new(ending).color(theme::text_faint()))).clicked() {
                        picks.push(Pick::Command(Command::NewAs(index)));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Change this list\u{2026}").clicked() {
                    picks.push(Pick::Command(Command::EditNewKinds));
                    ui.close();
                }
            }
            Live::RecentProjects => {
                if self.recent_projects.is_empty() {
                    empty(ui, "Projects you open will be listed here.");
                }
                for path in &self.recent_projects {
                    if ui.button(name_of(path)).on_hover_text(path.display().to_string()).clicked() {
                        picks.push(Pick::Project(path.clone()));
                        ui.close();
                    }
                }
            }
            Live::Macros | Live::Scripts => {
                self.library.refresh_if_stale();
                let items = if live == Live::Macros { &self.library.macros } else { &self.library.scripts };
                if items.is_empty() {
                    empty(ui, if live == Live::Macros { "No saved macros yet." } else { "No scripts yet." });
                }
                for item in items {
                    if ui.add_enabled(!self.no_file() || live == Live::Scripts, Button::new(item.name.as_str())).clicked() {
                        picks.push(Pick::Play(item.path.clone()));
                        ui.close();
                    }
                }
            }
            Live::WindowThemes => {
                let names: Vec<String> = self.themes.windows.iter().map(|t| t.name.clone()).collect();
                for name in names {
                    if ui.radio(self.themes.window == name, name.as_str()).clicked() {
                        picks.push(Pick::WindowTheme(name));
                        ui.close();
                    }
                }
            }
            Live::DocumentThemes => {
                let names: Vec<String> = self.themes.documents.iter().map(|t| t.name.clone()).collect();
                if !self.themes.rules.is_empty() {
                    empty(ui, "For documents no rule picks a theme for. Themes and rules sets the rules.");
                }
                for name in names {
                    if ui.radio(self.themes.document == name, name.as_str()).clicked() {
                        picks.push(Pick::DocumentTheme(name));
                        ui.close();
                    }
                }
            }
            Live::ThisDocumentTheme => {
                let usable = !self.no_file();
                let own = self.docs[self.active].theme_override.clone();
                if ui.add_enabled(usable, egui::RadioButton::new(own.is_none(), "As the rules say")).clicked() {
                    picks.push(Pick::ThisDocumentTheme(None));
                    ui.close();
                }
                let names: Vec<String> = self.themes.documents.iter().map(|t| t.name.clone()).collect();
                for name in names {
                    if ui.add_enabled(usable, egui::RadioButton::new(own.as_deref() == Some(name.as_str()), name.as_str())).clicked() {
                        picks.push(Pick::ThisDocumentTheme(Some(name)));
                        ui.close();
                    }
                }
            }
            Live::Devices => {
                if self.sends.devices.is_empty() {
                    empty(ui, "Devices you set up are listed here. Manage devices adds one: a card, a folder, a server, a Spectrum Next or an Ultimate 64.");
                }
                let usable = !self.no_file() && self.docs[self.active].browser.is_none();
                for number in 0..self.sends.devices.len() {
                    let device = self.sends.devices[number].clone();
                    let absent = matches!(&device.place, crate::deploy::Place::Card { label, .. } if self.sends.card_present(label) == Some(false));
                    let label = if absent { format!("{}  (not plugged in)", device.name) } else { device.name.clone() };
                    if ui.add_enabled(usable, Button::new(label)).on_hover_text(device.place.describe()).clicked() {
                        picks.push(Pick::Device(number));
                        ui.close();
                    }
                }
            }
            Live::Machines => {
                // What the file in front would run as, so each machine says whether it can take it.
                let runnable = if self.no_file() { None } else { self.runnable_quietly().ok().flatten() };
                let mut targets = vec![(super::run_host::Target::BuiltIn, super::run_host::BUILT_IN_NAME.to_string(), Pick::RunOn(None))];
                targets.extend((0..self.sends.devices.len()).map(|n| (super::run_host::Target::Device(n), self.sends.devices[n].name.clone(), Pick::RunOn(Some(n)))));
                for (target, name, pick) in targets {
                    let suits = runnable.as_ref().map(|run| self.suits(target, run));
                    let button = ui.add_enabled(!matches!(suits, Some(Err(_))), Button::new(name));
                    let button = match &suits {
                        Some(Err(why)) => button.on_disabled_hover_text(why.as_str()),
                        _ => button,
                    };
                    if button.clicked() {
                        picks.push(pick);
                        ui.close();
                    }
                }
            }
            Live::AddOns => {
                self.addons.refresh_if_stale();
                let commands = self.addons.commands();
                if commands.is_empty() {
                    empty(ui, "Add-ons that are on list their commands here. Manage add-ons turns them on.");
                }
                for (number, listed) in commands.iter().enumerate() {
                    if ui.button(listed.title.as_str()).on_hover_text(format!("From {}", listed.addon)).clicked() {
                        picks.push(Pick::AddOn(number));
                        ui.close();
                    }
                }
            }
            Live::Language => {
                let doc = &self.docs[self.active];
                let usable = !doc.hex_mode && !self.no_file() && highlight::engine().is_some();
                ui.add_enabled_ui(usable, |ui| {
                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                        if ui.radio(doc.language == Language::Auto, "Work it out").clicked() {
                            picks.push(Pick::Language(Language::Auto));
                        }
                        if ui.radio(doc.language == Language::Off, "Plain text").clicked() {
                            picks.push(Pick::Language(Language::Off));
                        }
                        ui.separator();
                        if let Some(engine) = highlight::engine() {
                            for (name, syntax) in engine.names() {
                                if ui.radio(doc.language == Language::Named(*syntax), name).clicked() {
                                    picks.push(Pick::Language(Language::Named(*syntax)));
                                }
                            }
                        }
                    });
                });
            }
            Live::Snippets => {
                if self.snippets.is_empty() {
                    empty(ui, "Snippets you make will be listed here.");
                }
                let usable = !self.no_file() && !self.docs[self.active].hex_mode;
                for (index, snippet) in self.snippets.iter().enumerate() {
                    let preview: String = snippet.body.chars().take(200).collect();
                    if ui.add_enabled(usable, Button::new(snippet.name.as_str())).on_hover_text(preview).clicked() {
                        picks.push(Pick::Snippet(index));
                        ui.close();
                    }
                }
            }
            Live::Tags => {
                let text = &self.docs[self.active].text;
                let tags = if self.docs[self.active].hex_mode || self.no_file() { Vec::new() } else { text.tags() };
                if tags.is_empty() {
                    empty(ui, "Sections you tag with a colour will be listed here.");
                }
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    for (first, last, colour) in tags {
                        let words: String = text.format.readable_text(&text.line(first)).trim().chars().take(48).collect();
                        let mut job = egui::text::LayoutJob::default();
                        RichText::new("\u{25A0}  ").color(theme::tag_colour(colour)).append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                        RichText::new(format!("{}  ", first + 1)).color(theme::text_faint()).append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                        RichText::new(if words.is_empty() { "(blank line)".to_string() } else { words }).append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                        if ui.button(job).on_hover_text(format!("Lines {} to {}", first + 1, last + 1)).clicked() {
                            picks.push(Pick::Line(first));
                            ui.close();
                        }
                    }
                });
            }
            Live::TabSize => {
                ui.horizontal(|ui| {
                    ui.label(label);
                    ui.add(egui::DragValue::new(&mut self.view.tab_size).range(text_buffer::MIN_TAB_SIZE..=text_buffer::MAX_TAB_SIZE)).on_hover_text("Columns from one tab stop to the next, for all open files");
                });
            }
        }
        let _ = chosen;
    }

    fn carry_out(&mut self, picks: Vec<Pick>) {
        for pick in picks {
            match pick {
                Pick::File(path) => self.open_request = Some(path),
                Pick::Project(path) => self.open_project(&path),
                Pick::Language(language) => self.doc().set_language(language),
                Pick::Snippet(index) => self.insert_snippet(index),
                Pick::Line(line) => {
                    let text = &mut self.doc().text;
                    text.move_caret(line, 0, false);
                    text.reveal = true;
                    self.pending_focus = true;
                }
                Pick::AddOn(number) => self.pending_command = Some(Command::AddOn(number)),
                Pick::Device(number) => self.pending_command = Some(Command::SendTo(number)),
                Pick::Command(command) => self.pending_command = Some(command),
                Pick::RunOn(None) => self.pending_command = Some(Command::RunOnBuiltIn),
                Pick::RunOn(Some(number)) => self.pending_command = Some(Command::RunOn(number)),
                Pick::WindowTheme(name) => {
                    self.themes.window = name;
                    self.save_settings_if_changed();
                }
                Pick::DocumentTheme(name) => {
                    self.themes.document = name;
                    self.save_settings_if_changed();
                }
                Pick::ThisDocumentTheme(name) => self.doc().theme_override = name,
                Pick::Play(path) => {
                    if self.library.macros.iter().any(|item| item.path == path) {
                        self.current_macro = Some(script_host::CurrentMacro { name: path.file_stem().map_or_else(String::new, |n| n.to_string_lossy().into_owned()), path: Some(path.clone()), source: String::new() });
                    }
                    self.pending_play = Some((Some(path), scripting::Repeat::Once));
                }
            }
        }
    }


    /// The bar along the top: the menus, and the box that finds any command.
    pub(super) fn top_bar(&mut self, ui: &mut Ui) -> Option<Command> {
        let mut chosen = None;
        let mut picks = Vec::new();
        let mut first_menu_left = 0.0;
        egui::MenuBar::new().ui(ui, |ui| {
            ui.spacing_mut().button_padding = egui::vec2(9.0, 4.0);
            first_menu_left = ui.cursor().min.x;
            for (title, entries) in MENUS {
                ui.menu_button(title, |ui| {
                    ui.set_min_width(MENU_WIDTH);
                    self.draw_entries(ui, entries, false, &mut chosen, &mut picks);
                });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if command_box(ui).clicked() {
                    chosen = Some(Command::CommandPalette);
                }
            });
        });
        // For a picture, or a test: one menu laid open under the bar.
        if let Some((index, opened_out)) = self.shown_menu
            && let Some((_, entries)) = MENUS.get(index)
        {
            let top = ui.min_rect().max.y;
            egui::Area::new(ui.scope_id().with("shown-menu")).order(egui::Order::Foreground).fixed_pos(egui::pos2(first_menu_left + index as f32 * 52.0, top)).show(ui.ctx(), |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    // As a menu opened by a click draws itself.
                    ui.style_mut().visuals.button_frame = false;
                    ui.set_min_width(MENU_WIDTH);
                    self.drawing_open = true;
                    self.draw_entries(ui, entries, opened_out, &mut chosen, &mut picks);
                    self.drawing_open = false;
                });
            });
        }
        self.carry_out(picks);
        chosen
    }

    // ---------- The command box ----------

    pub(super) fn palette_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::Palette(state) = &mut self.dialog else {
            return false;
        };
        let (up, down, enter, escape) = ctx.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown), i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
        let query = state.query.clone();
        let mut all = every_command();
        self.addons.refresh_if_stale();
        all.extend(self.sends.devices.iter().enumerate().map(|(number, device)| Found { path: format!("File \u{203A} Send to device \u{203A} {}", device.name), shortcut: "", command: Command::SendTo(number) }));
        all.push(Found { path: format!("File \u{203A} Run on \u{203A} {}", super::run_host::BUILT_IN_NAME), shortcut: "", command: Command::RunOnBuiltIn });
        all.extend(self.sends.devices.iter().enumerate().map(|(number, device)| Found { path: format!("File \u{203A} Run on \u{203A} {}", device.name), shortcut: "", command: Command::RunOn(number) }));
        all.extend(self.addons.commands().into_iter().enumerate().map(|(number, listed)| Found { path: format!("Tools \u{203A} Add-ons \u{203A} {} \u{203A} {}", listed.addon, listed.title), shortcut: "", command: Command::AddOn(number) }));
        let found = matching(&query, all);
        let usable: Vec<bool> = found.iter().map(|each| self.can(each.command)).collect();
        let states: Vec<Option<bool>> = found.iter().map(|each| if let Command::Toggle(flag) = each.command { Some(self.flag_on(flag)) } else { None }).collect();
        let Dialog::Palette(state) = &mut self.dialog else {
            return false;
        };
        if down {
            state.selected = (state.selected + 1).min(found.len().saturating_sub(1));
        }
        if up {
            state.selected = state.selected.saturating_sub(1);
        }
        state.selected = state.selected.min(found.len().saturating_sub(1));
        let mut run = None;
        egui::Window::new("Find a command").title_bar(false).collapsible(false).resizable(false).fixed_size(egui::vec2(560.0, 0.0)).anchor(egui::Align2::CENTER_TOP, [0.0, 64.0]).show(ctx, |ui| {
            let field = ui.add(egui::TextEdit::singleline(&mut state.query).hint_text("Type what you want to do, such as \u{201C}sort\u{201D} or \u{201C}wrap\u{201D}").desired_width(f32::INFINITY).margin(egui::vec2(10.0, 8.0)).font(egui::FontId::proportional(16.0)));
            if state.focus {
                field.request_focus();
                state.focus = false;
            }
            if field.changed() {
                state.selected = 0;
            }
            ui.add_space(6.0);
            if found.is_empty() {
                ui.label(RichText::new("Nothing matches that. Try another word.").color(theme::text_faint()));
            }
            egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                for (index, each) in found.iter().enumerate() {
                    let here = index == state.selected;
                    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::click());
                    if here || response.hovered() {
                        ui.painter().rect_filled(rect, 6.0, if here { theme::selected() } else { theme::hover() });
                    }
                    if here {
                        response.scroll_to_me(None);
                    }
                    let (place, name) = each.path.rsplit_once(" \u{203A} ").unwrap_or(("", each.path.as_str()));
                    let colour = if usable[index] { theme::text() } else { theme::text_disabled() };
                    let left = rect.min.x + 12.0;
                    let galley = ui.painter().layout_no_wrap(name.to_string(), egui::FontId::proportional(14.0), colour);
                    let name_width = galley.size().x;
                    ui.painter().galley(egui::pos2(left, rect.center().y - galley.size().y / 2.0), galley, colour);
                    ui.painter().text(egui::pos2(left + name_width + 10.0, rect.center().y), egui::Align2::LEFT_CENTER, place.replace(" \u{203A} ", " / "), egui::FontId::proportional(12.0), theme::text_faint());
                    let right = match states[index] {
                        Some(true) => "On".to_string(),
                        Some(false) => "Off".to_string(),
                        None => shown_shortcut(each.shortcut),
                    };
                    ui.painter().text(egui::pos2(rect.max.x - 12.0, rect.center().y), egui::Align2::RIGHT_CENTER, right, egui::FontId::proportional(12.0), theme::text_faint());
                    if response.clicked() && usable[index] {
                        run = Some(each.command);
                    }
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new("\u{2191} \u{2193} to choose, Enter to do it, Esc to close").color(theme::text_faint()).size(11.5));
        });
        if enter && let Some(index) = found.get(state.selected).map(|_| state.selected) && usable[index] {
            run = Some(found[index].command);
        }
        if escape || run.is_some() {
            self.close_dialog();
        }
        self.pending_command = run;
        true
    }

    pub(super) fn about(&mut self) {
        let notices = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|folder| folder.join("THIRD-PARTY-NOTICES.txt"))).filter(|path| path.is_file());
        self.dialog = Dialog::About { notices };
    }
}

/// Looks like a search box, and opens the command box.
fn command_box(ui: &mut Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(250.0, 24.0), egui::Sense::click());
    let back = if response.hovered() { theme::hover() } else { theme::field() };
    ui.painter().rect(rect, 7.0, back, egui::Stroke::new(1.0, theme::border()), egui::StrokeKind::Inside);
    ui.painter().text(egui::pos2(rect.min.x + 10.0, rect.center().y), egui::Align2::LEFT_CENTER, "Find a command\u{2026}", egui::FontId::proportional(13.0), theme::text_faint());
    ui.painter().text(egui::pos2(rect.max.x - 10.0, rect.center().y), egui::Align2::RIGHT_CENTER, "F1", egui::FontId::proportional(12.0), theme::text_faint());
    response.on_hover_text("Find any command by what it does")
}

#[cfg(test)]
mod mac_tests {
    #[test]
    fn shortcuts_are_shown_the_macs_way_there() {
        for (pc, mac) in [("Ctrl+S", "\u{2318}S"), ("Ctrl+Shift+S", "\u{21E7}\u{2318}S"), ("Ctrl+Alt+H", "\u{2325}\u{2318}H"), ("Alt+U", "\u{2303}\u{2325}U"), ("Alt+Left", "\u{2318}["), ("Ctrl+Tab", "\u{2303}Tab"), ("F6", "F6"), ("Shift+F3", "\u{21E7}F3")] {
            assert_eq!(super::mac_shortcut(pc), mac, "{pc}");
        }
    }
}
