//! The ribbon, as Word and UltraEdit have: tabs of large and small buttons with pictures, in place of the menus, for
//! those who find a command faster by sight than by name. It's switched on and off by the switch at the top right,
//! beside Find a command, from View, or in Settings, Appearance.
//!
//! It's made from the very same commands as the menus, and asks the same questions of them (`can` and `flag_on`), so
//! a button is always enabled, ticked or greyed exactly as its menu item is; its drop-down lists are the menus' own
//! lists, drawn by the menus' own code. It folds to its tabs (Ctrl+F1, or a double-click on a tab), shows each
//! command's keys when the pointer rests on it, and Alt puts a letter on each tab and then each button, to reach
//! them from the keyboard as Office does.

use super::menus::{self, Entry, Flag, Live, PanelFlag};
use super::*;
use crate::ribbon_icons::{self, Icon};
use egui::{Id, Popup, Pos2, Rect, Sense, Stroke, pos2, vec2};

/// What a control does.
#[derive(Clone, Copy)]
pub(super) enum What {
    Run(Command),
    Tick(Flag),
    /// A list that drops down, drawn as the menus draw it.
    Drop(&'static [Entry]),
}

/// A button on the ribbon: its words, its picture, what it does, and whether it's one of the large ones.
#[derive(Clone, Copy)]
pub(super) struct Control {
    pub label: &'static str,
    pub icon: Icon,
    pub what: What,
    pub big: bool,
}

const fn big(label: &'static str, icon: Icon, command: Command) -> Control {
    Control { label, icon, what: What::Run(command), big: true }
}

const fn small(label: &'static str, icon: Icon, command: Command) -> Control {
    Control { label, icon, what: What::Run(command), big: false }
}

const fn tick(label: &'static str, icon: Icon, flag: Flag) -> Control {
    Control { label, icon, what: What::Tick(flag), big: false }
}

const fn drop(label: &'static str, icon: Icon, entries: &'static [Entry]) -> Control {
    Control { label, icon, what: What::Drop(entries), big: false }
}

const fn big_drop(label: &'static str, icon: Icon, entries: &'static [Entry]) -> Control {
    Control { label, icon, what: What::Drop(entries), big: true }
}

/// Buttons that belong together, with a name under them.
pub(super) struct Group {
    pub name: &'static str,
    pub controls: &'static [Control],
}

/// A tab of the ribbon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Home,
    Edit,
    Search,
    View,
    Format,
    Code,
    Retro,
    Tools,
    /// The tab for the kind of file in front, there while it's in front.
    Kind(Kind),
}

/// The kinds of file that bring a tab of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Assembly,
    Basic,
    Picture,
    Hex,
    Table,
    Markdown,
    Document,
}

impl Kind {
    #[cfg(test)]
    pub const ALL: [Self; 7] = [Self::Assembly, Self::Basic, Self::Picture, Self::Hex, Self::Table, Self::Markdown, Self::Document];
}

impl Tab {
    /// The tabs that are always there, in order.
    pub const FIXED: [Self; 8] = [Self::Home, Self::Edit, Self::Search, Self::View, Self::Format, Self::Code, Self::Retro, Self::Tools];

    pub fn name(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Edit => "Edit",
            Self::Search => "Search",
            Self::View => "View",
            Self::Format => "Format",
            Self::Code => "Code",
            Self::Retro => "Retro",
            Self::Tools => "Tools",
            Self::Kind(Kind::Assembly) => "Assembly",
            Self::Kind(Kind::Basic) => "BASIC",
            Self::Kind(Kind::Picture) => "Picture",
            Self::Kind(Kind::Hex) => "Hex",
            Self::Kind(Kind::Table) => "Table",
            Self::Kind(Kind::Markdown) => "Markdown",
            Self::Kind(Kind::Document) => "Document",
        }
    }

    /// The letter Alt shows on the tab, as Office's are: the same in every language, so they can be learnt.
    pub fn key(self) -> char {
        match self {
            Self::Home => 'H',
            Self::Edit => 'E',
            Self::Search => 'S',
            Self::View => 'W',
            Self::Format => 'O',
            Self::Code => 'C',
            Self::Retro => 'R',
            Self::Tools => 'T',
            Self::Kind(_) => 'X',
        }
    }

    pub fn groups(self) -> &'static [Group] {
        match self {
            Self::Home => HOME,
            Self::Edit => EDIT,
            Self::Search => SEARCH,
            Self::View => VIEW,
            Self::Format => FORMAT,
            Self::Code => CODE,
            Self::Retro => RETRO,
            Self::Tools => TOOLS,
            Self::Kind(Kind::Assembly) => ASSEMBLY,
            Self::Kind(Kind::Basic) => BASIC,
            Self::Kind(Kind::Picture) => PICTURE,
            Self::Kind(Kind::Hex) => HEX,
            Self::Kind(Kind::Table) => TABLE,
            Self::Kind(Kind::Markdown) => MARKDOWN,
            Self::Kind(Kind::Document) => DOCUMENT,
        }
    }
}

/// The File button's letter, before the tabs'.
const FILE_KEY: char = 'F';

const NEW_AS: &[Entry] = &[Entry::Live("New as", Live::NewAs)];
const OPEN_RECENT: &[Entry] = &[Entry::Live("Open recent", Live::RecentFiles)];
/// What the File button holds: the File menu, then the Window and Help menus, which have no tab of their own.
const FILE_BUTTON: &[Entry] = &[Entry::Line, Entry::Sub("Window", menus::WINDOW), Entry::Sub("Help", menus::HELP_MENU)];

const HOME: &[Group] = &[
    Group { name: "Clipboard", controls: &[big("Paste", Icon::Paste, Command::Paste), small("Cut", Icon::Cut, Command::Cut), small("Copy", Icon::Copy, Command::Copy), drop("Clipboards", Icon::Text("1-9"), menus::CLIPBOARDS)] },
    Group {
        name: "File",
        controls: &[
            big("New", Icon::New, Command::New),
            big("Open\u{2026}", Icon::Open, Command::Open),
            small("Save", Icon::Save, Command::Save),
            small("Save as\u{2026}", Icon::SaveAs, Command::SaveAs),
            drop("New as", Icon::New, NEW_AS),
            drop("Open recent", Icon::Clock, OPEN_RECENT),
            small("Open folder\u{2026}", Icon::Open, Command::OpenFolder),
            small("Print\u{2026}", Icon::Print, Command::Print),
        ],
    },
    Group { name: "Undo", controls: &[small("Undo", Icon::Undo, Command::Undo), small("Redo", Icon::Redo, Command::Redo), small("Select all", Icon::SelectAll, Command::SelectAll)] },
    Group { name: "Find", controls: &[big("Find\u{2026}", Icon::Find, Command::Find), small("Replace\u{2026}", Icon::Replace, Command::Replace), small("Go to line\u{2026}", Icon::GoTo, Command::GoTo), small("Go to file\u{2026}", Icon::Open, Command::GoToFile)] },
    Group { name: "Show", controls: &[tick("Word wrap", Icon::Wrap, Flag::WordWrap), tick("Line numbers", Icon::Text("12"), Flag::LineNumbers), tick("Document map", Icon::Map, Flag::DocumentMap)] },
    Group { name: "Run", controls: &[big("Run on machine", Icon::Run, Command::RunOnMachine), small("Run a command\u{2026}", Icon::Terminal, Command::RunTool), small("Stop it", Icon::Stop, Command::StopTool), small("Terminal", Icon::Terminal, Command::ToggleTerminal)] },
];

const EDIT: &[Group] = &[
    Group { name: "Undo", controls: &[big("Undo", Icon::Undo, Command::Undo), big("Redo", Icon::Redo, Command::Redo), small("Edit history", Icon::Clock, Command::ShowHistory), small("History of this part", Icon::Clock, Command::PartHistory)] },
    Group { name: "Clipboard", controls: &[big("Paste", Icon::Paste, Command::Paste), small("Cut", Icon::Cut, Command::Cut), small("Copy", Icon::Copy, Command::Copy), small("Swap with clipboard", Icon::Replace, Command::SwapClipboard), drop("Clipboards", Icon::Text("1-9"), menus::CLIPBOARDS)] },
    Group { name: "Select", controls: &[small("Select all", Icon::SelectAll, Command::SelectAll), small("Every copy of the selection", Icon::SelectAll, Command::MakeSelections), drop("Select", Icon::SelectAll, menus::SELECT)] },
    Group {
        name: "Lines",
        controls: &[
            small("Move up", Icon::Up, Command::MoveLineUp),
            small("Move down", Icon::Down, Command::MoveLineDown),
            small("Duplicate", Icon::Copy, Command::Duplicate),
            small("Indent", Icon::Indent, Command::Indent),
            small("Outdent", Icon::Outdent, Command::Outdent),
            drop("Lines", Icon::Lines, menus::LINES),
        ],
    },
    Group { name: "Change", controls: &[drop("Change case", Icon::Text("Aa"), menus::CHANGE_CASE), drop("Insert", Icon::Insert, menus::INSERT), drop("Surround with", Icon::Text("( )"), menus::SURROUND), drop("Colour tag", Icon::Tag, menus::COLOUR_TAG)] },
    Group { name: "Columns", controls: &[tick("Column mode", Icon::Columns, Flag::ColumnMode), drop("Columns", Icon::Columns, menus::COLUMNS), tick("Read-only", Icon::Lock, Flag::ReadOnly)] },
    Group { name: "Everything", controls: &[big_drop("More", Icon::More, menus::EDIT)] },
];

const SEARCH: &[Group] = &[
    Group {
        name: "Find",
        controls: &[
            big("Find\u{2026}", Icon::Find, Command::Find),
            big("Replace\u{2026}", Icon::Replace, Command::Replace),
            small("Find next", Icon::Down, Command::FindNext),
            small("Find previous", Icon::Up, Command::FindPrevious),
            small("Only show lines with the selection", Icon::Lines, Command::FilterSelection),
        ],
    },
    Group { name: "Folder", controls: &[big("Search panel", Icon::Find, Command::SearchPanel), small("Find in files\u{2026}", Icon::Open, Command::FindInFiles), small("Go to file\u{2026}", Icon::Open, Command::GoToFile)] },
    Group {
        name: "Go to",
        controls: &[
            big("Go to line\u{2026}", Icon::GoTo, Command::GoTo),
            small("Go to symbol in this file\u{2026}", Icon::Text("f()"), Command::GoToSymbol(false)),
            small("Go to symbol in the folder\u{2026}", Icon::Text("f()"), Command::GoToSymbol(true)),
            small("Go to the matching bracket", Icon::Text("{}"), Command::GoToBrace),
            small("Go to definition", Icon::Text("f()"), Command::GoToDefinition),
            small("Find all uses", Icon::Find, Command::FindUses),
        ],
    },
    Group { name: "Places", controls: &[small("Back", Icon::Back, Command::NavBack), small("Forward", Icon::Forward, Command::NavForward), small("Previous change", Icon::Up, Command::LastChange), small("Next change", Icon::Down, Command::NextChange)] },
    Group { name: "Bookmarks", controls: &[big("Set or clear", Icon::Bookmark, Command::ToggleBookmark), small("Next", Icon::Down, Command::NextBookmark), small("Previous", Icon::Up, Command::PreviousBookmark), drop("Bookmarks", Icon::Bookmark, menus::BOOKMARKS)] },
    Group { name: "Colour tags", controls: &[small("Next", Icon::Down, Command::NextTag), small("Previous", Icon::Up, Command::PreviousTag), drop("Colour tag", Icon::Tag, menus::COLOUR_TAG)] },
    Group { name: "Everything", controls: &[big_drop("More", Icon::More, menus::SEARCH)] },
];

const VIEW: &[Group] = &[
    Group { name: "Text", controls: &[small("Larger text", Icon::Plus, Command::ZoomIn), small("Smaller text", Icon::Minus, Command::ZoomOut), small("Font\u{2026}", Icon::Text("Aa"), Command::ChooseFont)] },
    Group {
        name: "Show",
        controls: &[
            tick("Line numbers", Icon::Text("12"), Flag::LineNumbers),
            tick("Ruler", Icon::Text("|||"), Flag::Ruler),
            tick("Document map", Icon::Map, Flag::DocumentMap),
            tick("Breadcrumbs", Icon::Forward, Flag::Breadcrumbs),
            tick("Spaces and tabs", Icon::Text("\u{B7}\u{B7}"), Flag::Whitespace),
            tick("Rainbow brackets", Icon::Text("{}"), Flag::Rainbow),
            drop("Show", Icon::Eye, menus::SHOW),
        ],
    },
    Group { name: "Wrap and fold", controls: &[tick("Word wrap", Icon::Wrap, Flag::WordWrap), drop("Folding", Icon::Minus, menus::FOLDING), small("Narrow to the selection", Icon::Narrow, Command::Narrow), small("Show the whole file", Icon::Lines, Command::Widen)] },
    Group { name: "Views", controls: &[big("As hex", Icon::Text("0x"), Command::ViewHex), small("As text", Icon::Lines, Command::ViewRegular), drop("Preview", Icon::Eye, menus::PREVIEW), drop("Split", Icon::Split, menus::SPLIT)] },
    Group { name: "Panels", controls: &[tick("Project", Icon::Panel, Flag::Panel(PanelFlag::Project)), tick("Functions", Icon::Panel, Flag::Panel(PanelFlag::Functions)), tick("Output", Icon::Terminal, Flag::OutputWindow), drop("Panels", Icon::Panel, menus::PANELS), small("Close every panel", Icon::Window, Command::HidePanels)] },
    Group {
        name: "Window",
        controls: &[
            tick("File tabs", Icon::Window, Flag::FileTabs),
            tick("Status bar", Icon::Window, Flag::StatusBar),
            tick("Ribbon", Icon::Ribbon, Flag::Ribbon),
            tick("Full screen", Icon::FullScreen, Flag::FullScreen),
            tick("Always on top", Icon::Pin, Flag::AlwaysOnTop),
            small("Fold the ribbon to its tabs", Icon::Fold, Command::FoldRibbon),
        ],
    },
    Group { name: "Theme", controls: &[big_drop("Theme", Icon::Theme, menus::THEME), small("Window font and size\u{2026}", Icon::Text("Aa"), Command::WindowFont)] },
    Group { name: "Everything", controls: &[big_drop("More", Icon::More, menus::VIEW)] },
];

const FORMAT: &[Group] = &[
    Group { name: "Encoding", controls: &[big_drop("Encoding", Icon::Text("\u{C4}"), menus::ENCODING)] },
    Group { name: "Indent", controls: &[small("Reindent", Icon::Indent, Command::Reindent), small("Indent", Icon::Indent, Command::Indent), small("Outdent", Icon::Outdent, Command::Outdent), tick("Tab key types spaces", Icon::Forward, Flag::SpacesForTab), drop("Tabs and spaces", Icon::Lines, menus::TABS_AND_SPACES)] },
    Group {
        name: "Paragraph",
        controls: &[
            small("Align left", Icon::AlignLeft, Command::AlignText(Align::Left)),
            small("Centre", Icon::AlignCentre, Command::AlignText(Align::Center)),
            small("Align right", Icon::AlignRight, Command::AlignText(Align::Right)),
            small("Justify", Icon::Justify, Command::AlignText(Align::Justify)),
            small("Reformat", Icon::Wrap, Command::ParagraphReformat),
            drop("Paragraph", Icon::Lines, menus::PARAGRAPH),
        ],
    },
    Group {
        name: "Code",
        controls: &[
            big("Format document", Icon::Indent, Command::FormatDocument),
            small("Comment out", Icon::Text("//"), Command::Comment),
            small("Uncomment", Icon::Text("//"), Command::Uncomment),
            small("Doc comment for this function", Icon::Text("/**"), Command::DocComment),
            drop("Lay out code", Icon::Text("{}"), menus::LAY_OUT_CODE),
            drop("Compress", Icon::Text("{}"), menus::COMPRESS),
        ],
    },
    Group { name: "Convert", controls: &[drop("Convert", Icon::Replace, menus::CONVERT), drop("Table", Icon::Table, menus::TABLE), drop("Save the document as", Icon::SaveAs, menus::SAVE_DOCUMENT_AS)] },
    Group { name: "Markdown", controls: &[small("Markdown: table of contents", Icon::Lines, Command::MarkdownToc), small("Markdown: line up the table", Icon::Table, Command::MarkdownTable)] },
    Group { name: "Everything", controls: &[big_drop("More", Icon::More, menus::FORMAT)] },
];

const CODE: &[Group] = &[
    Group {
        name: "Run",
        controls: &[
            big("Run a command\u{2026}", Icon::Run, Command::RunTool),
            small("Stop it", Icon::Stop, Command::StopTool),
            small("Debug (Python, C, C++, Rust)", Icon::Debug, Command::StartDebugging),
            small("Build", Icon::Build, Command::BuildTask),
            small("Test", Icon::Test, Command::TestTask),
            drop("Tasks", Icon::Build, menus::TASKS),
        ],
    },
    Group {
        name: "Code",
        controls: &[
            big("Go to definition", Icon::Text("f()"), Command::GoToDefinition),
            small("Peek definition", Icon::Eye, Command::PeekDefinition),
            small("Find all uses", Icon::Find, Command::FindUses),
            small("Rename\u{2026}", Icon::Text("ab"), Command::RenameLabel),
            small("Quick fix\u{2026}", Icon::Bulb, Command::QuickFix),
            small("Format document", Icon::Indent, Command::FormatDocument),
        ],
    },
    Group { name: "Comments", controls: &[small("Comment out", Icon::Text("//"), Command::Comment), small("Uncomment", Icon::Text("//"), Command::Uncomment), small("Doc comment for this function", Icon::Text("/**"), Command::DocComment)] },
    Group {
        name: "Git",
        controls: &[
            big("Source control", Icon::Git, Command::SourceControl),
            small("Compare with the last commit", Icon::Compare, Command::GitCompareHead),
            small("History of this file\u{2026}", Icon::Clock, Command::GitHistory),
            tick("Show who changed each line", Icon::Text("@"), Flag::GitBlame),
            small("Copy a project from online (clone)\u{2026}", Icon::Download, Command::GitClone),
            drop("Git", Icon::Git, menus::GIT),
        ],
    },
    Group { name: "Folder", controls: &[big("Open folder\u{2026}", Icon::Open, Command::OpenFolder), small("Go to file\u{2026}", Icon::Open, Command::GoToFile), small("Search panel", Icon::Find, Command::SearchPanel), drop("Project", Icon::Book, menus::PROJECT)] },
    Group { name: "Assistant", controls: &[tick("Assistant panel", Icon::Assistant, Flag::AssistantPanel), small("Fix\u{2026}", Icon::Assistant, Command::Assist(assistant::Task::Fix)), small("Explain\u{2026}", Icon::Assistant, Command::Assist(assistant::Task::Explain)), drop("Assistant", Icon::Assistant, menus::ASSISTANT)] },
    Group { name: "Terminal", controls: &[big("Terminal", Icon::Terminal, Command::ToggleTerminal), small("Serve this folder in a browser (live server)", Icon::Web, Command::ServeFolder)] },
];

const RETRO: &[Group] = &[
    Group { name: "Run", controls: &[big("Run on machine", Icon::Run, Command::RunOnMachine), drop("Run on", Icon::Chip, menus::RUN_ON), drop("Send to device", Icon::Send, menus::SEND_TO), small("Stop running", Icon::Stop, Command::StopRunning)] },
    Group {
        name: "Assembler",
        controls: &[
            small("Assemble and save", Icon::Chip, Command::AssembleAndSave),
            small("Assemble and save as binary\u{2026}", Icon::Text("01"), Command::AssembleBinary),
            small("Assemblers\u{2026}", Icon::Settings, Command::ChooseAssembler),
            small("Disassemble\u{2026}", Icon::Text("01"), Command::Disassemble),
            tick("Cycle counts beside 6502, Z80, 6809 and 68000 code", Icon::Clock, Flag::CycleCounts),
        ],
    },
    Group {
        name: "Graphics",
        controls: &[
            big("Character and sprite editor", Icon::Sprite, Command::PixelEditor),
            small("Tile map and level editor", Icon::Tiles, Command::TileMapEditor),
            small("Picture converter", Icon::Picture, Command::RetroPictures),
            small("Screen designer", Icon::Picture, Command::ScreenDesigner),
        ],
    },
    Group { name: "Sound", controls: &[small("Sound effects", Icon::Sound, Command::SoundEffects), small("Ripper", Icon::Find, Command::Ripper)] },
    Group {
        name: "Machines",
        controls: &[
            drop("Amiga", Icon::Text("A"), menus::AMIGA),
            drop("Atari ST", Icon::Text("ST"), menus::ATARI_ST),
            drop("Commodore 64", Icon::Text("64"), menus::C64),
            drop("Atari 2600", Icon::Text("26"), menus::ATARI_2600),
            drop("ZX Spectrum and Next", Icon::Text("ZX"), menus::SPECTRUM),
            drop("General", Icon::Chip, menus::GENERAL),
            drop("Hardware", Icon::Chip, menus::HARDWARE),
        ],
    },
    Group {
        name: "Disks and ROMs",
        controls: &[
            big("New disk or tape image\u{2026}", Icon::Disc, Command::NewImage),
            small("Dragon DOS disk\u{2026}", Icon::Disc, Command::DragonDisk),
            small("Checksums and ROM headers", Icon::Text("#"), Command::Checksums),
            small("ROM patches: IPS, BPS, UPS", Icon::Chip, Command::RomPatches),
        ],
    },
    Group { name: "BASIC", controls: &[small("Open the listing as text", Icon::Lines, Command::OpenBasicListing), small("Spectrum GRAPH mode", Icon::Sprite, Command::GraphMode), drop("Tokenised BASIC", Icon::Save, menus::TOKENISED_BASIC)] },
];

const TOOLS: &[Group] = &[
    Group {
        name: "Compare",
        controls: &[
            big("Compare with\u{2026}", Icon::Compare, Command::Compare),
            small("Compare files\u{2026}", Icon::Compare, Command::CompareFiles),
            small("Compare folders\u{2026}", Icon::Open, Command::CompareFolders),
            small("Apply a patch\u{2026}", Icon::Paste, Command::ApplyPatch),
            small("Merge three files\u{2026}", Icon::Merge, Command::Merge),
            small("Saved comparisons\u{2026}", Icon::Save, Command::CompareSessions),
        ],
    },
    Group {
        name: "Measure",
        controls: &[
            small("Count words", Icon::Text("123"), Command::WordCount),
            small("Fingerprint (SHA-256)", Icon::Text("#"), Command::ComputeHash),
            small("Time spent\u{2026}", Icon::Clock, Command::TimeSpent),
            small("Convert a number", Icon::Text("0x"), Command::ConvertNumber),
            small("Regular expression tester", Icon::Text(".*"), Command::RegexTester),
            small("Colour as\u{2026}", Icon::Theme, Command::ColourAs),
        ],
    },
    Group { name: "Macros", controls: &[big("Record or stop", Icon::Record, Command::RecordMacro), small("Play", Icon::Run, Command::PlayMacro), small("Play several times\u{2026}", Icon::Run, Command::PlayMacroTimes), drop("Macros", Icon::Record, menus::MACROS)] },
    Group { name: "Scripts", controls: &[drop("Scripts", Icon::Script, menus::SCRIPTS), drop("Add-ons", Icon::Puzzle, menus::ADD_ONS), small("Code as a picture\u{2026}", Icon::Picture, Command::CodePicture)] },
    Group { name: "Settings", controls: &[big("Settings\u{2026}", Icon::Settings, Command::Settings), small("Save settings to a file\u{2026}", Icon::Save, Command::SaveSettingsFile), small("Restore settings from a file\u{2026}", Icon::Open, Command::RestoreSettingsFile)] },
    Group { name: "Help", controls: &[small("How things work", Icon::Book, Command::Help), small("What's new in this version", Icon::Star, Command::WhatsNew), drop("Guide", Icon::Book, menus::GUIDE)] },
    Group { name: "Everything", controls: &[big_drop("More", Icon::More, menus::TOOLS)] },
];

const ASSEMBLY: &[Group] = &[
    Group { name: "Assemble", controls: &[big("Assemble and save", Icon::Chip, Command::AssembleAndSave), small("Assemble and save as binary\u{2026}", Icon::Text("01"), Command::AssembleBinary), small("Assemblers\u{2026}", Icon::Settings, Command::ChooseAssembler)] },
    Group { name: "Run", controls: &[big("Run on machine", Icon::Run, Command::RunOnMachine), drop("Run on", Icon::Chip, menus::RUN_ON), small("Stop running", Icon::Stop, Command::StopRunning), small("Its memory", Icon::Text("0x"), Command::MachineMemory)] },
    Group {
        name: "Code",
        controls: &[
            small("Go to definition", Icon::Text("f()"), Command::GoToDefinition),
            small("Find all uses", Icon::Find, Command::FindUses),
            small("Rename\u{2026}", Icon::Text("ab"), Command::RenameLabel),
            tick("Cycle counts beside 6502, Z80, 6809 and 68000 code", Icon::Clock, Flag::CycleCounts),
            small("Disassemble\u{2026}", Icon::Text("01"), Command::Disassemble),
        ],
    },
];

const BASIC: &[Group] = &[
    Group { name: "Listing", controls: &[big("Open the listing as text", Icon::Lines, Command::OpenBasicListing), drop("Tokenised BASIC", Icon::Save, menus::TOKENISED_BASIC), small("Spectrum GRAPH mode", Icon::Sprite, Command::GraphMode), small("Spectrum UDG pictures from\u{2026}", Icon::Sprite, Command::UdgPictures)] },
    Group { name: "Run", controls: &[big("Run on machine", Icon::Run, Command::RunOnMachine), drop("Run on", Icon::Chip, menus::RUN_ON), small("Put this listing into it", Icon::Send, Command::MachineBasicIn), small("Take its BASIC program", Icon::Download, Command::MachineBasicOut)] },
];

const PICTURE: &[Group] = &[
    Group { name: "Picture", controls: &[big("Save the picture as PNG\u{2026}", Icon::Picture, Command::SavePictureAsPng), small("Character and sprite editor", Icon::Sprite, Command::PixelEditor), small("Picture converter", Icon::Picture, Command::RetroPictures), small("Ripper", Icon::Find, Command::Ripper)] },
    Group { name: "View", controls: &[small("As hex", Icon::Text("0x"), Command::ViewHex), small("As text", Icon::Lines, Command::ViewRegular)] },
];

const HEX: &[Group] = &[
    Group { name: "View", controls: &[big("As text", Icon::Lines, Command::ViewRegular), small("As hex", Icon::Text("0x"), Command::ViewHex), small("As hex, in EBCDIC", Icon::Text("0x"), Command::ViewEbcdic)] },
    Group {
        name: "Bytes",
        controls: &[
            small("Insert or delete bytes\u{2026}", Icon::Insert, Command::HexInsertDelete),
            small("Copy the selected rows", Icon::Copy, Command::HexCopySelected),
            small("Fingerprint (SHA-256)", Icon::Text("#"), Command::ComputeHash),
            small("Entropy and byte counts", Icon::Text("123"), Command::Entropy),
        ],
    },
    Group { name: "Panels", controls: &[tick("String explorer", Icon::Panel, Flag::Panel(PanelFlag::Strings)), tick("Value inspector", Icon::Panel, Flag::Panel(PanelFlag::Inspector)), tick("Structure", Icon::Panel, Flag::Panel(PanelFlag::Structure))] },
];

const TABLE: &[Group] = &[Group {
    name: "Table",
    controls: &[
        big("Table view (CSV, TSV)", Icon::Table, Command::ToggleTable),
        small("Comma-separated to fixed columns\u{2026}", Icon::Columns, Command::CsvToFixed),
        small("Fixed columns to comma-separated\u{2026}", Icon::Table, Command::FixedToCsv),
        small("Sort A to Z", Icon::Sort, Command::SortAscending),
        small("Sort Z to A", Icon::Sort, Command::SortDescending),
    ],
}];

const MARKDOWN: &[Group] = &[
    Group {
        name: "Preview",
        controls: &[
            small("Off", Icon::Lines, Command::SetPreview(settings::PreviewView::Text)),
            small("Beside the text", Icon::Split, Command::SetPreview(settings::PreviewView::Split)),
            small("Instead of the text", Icon::Eye, Command::SetPreview(settings::PreviewView::Full)),
        ],
    },
    Group { name: "Markdown", controls: &[small("Markdown: table of contents", Icon::Lines, Command::MarkdownToc), small("Markdown: line up the table", Icon::Table, Command::MarkdownTable)] },
    Group { name: "Spelling", controls: &[tick("Underline mistakes", Icon::Text("abc"), Flag::Spelling), small("Next mistake", Icon::Down, Command::NextMistake)] },
];

const DOCUMENT: &[Group] = &[Group {
    name: "Save the document as",
    controls: &[
        small("Plain text\u{2026}", Icon::Lines, Command::SaveDocumentAs(crate::legacy_docs::Target::Text)),
        small("Markdown\u{2026}", Icon::Text("#"), Command::SaveDocumentAs(crate::legacy_docs::Target::Markdown)),
        small("RTF\u{2026}", Icon::Text("Aa"), Command::SaveDocumentAs(crate::legacy_docs::Target::Rtf)),
        small("CSV, for a spreadsheet\u{2026}", Icon::Table, Command::SaveDocumentAs(crate::legacy_docs::Target::Csv)),
    ],
}];

/// Alt's letters on the ribbon: off, on the tabs, or on the chosen tab's buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tips {
    #[default]
    Off,
    Tabs,
    Buttons,
}

/// How the ribbon is now: the tab chosen, and what's shown of it.
#[derive(Default)]
pub(super) struct RibbonState {
    pub tab: Tab,
    /// Folded, but one tab's buttons shown over the text, until something's chosen or there's a click elsewhere.
    pub popped: bool,
    pub tips: Tips,
    /// Alt is held, and whether nothing else has been pressed with it, which makes letting go of it show the letters.
    alt_down: bool,
    alt_alone: bool,
    /// Where the bar of tabs ends, so a popped-out tab's buttons go just under it.
    bar_bottom: f32,
    /// Where the popped-out buttons were drawn, so a click outside them puts them away.
    popped_rect: Option<Rect>,
    /// The File button, for F10 and Alt's F.
    file_button: Option<Id>,
}

/// How much room the buttons are given: in full; the large ones made small; or pictures alone, their names on hover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fit {
    Full,
    Compact,
    Pictures,
}

const ROW: f32 = 24.0;
const BIG_ICON: f32 = 28.0;
const SMALL_ICON: f32 = 16.0;
const GROUP_NAME: f32 = 15.0;
/// The height of the buttons' part of the ribbon, from the top of a large button to the bottom of its group's name.
const BODY: f32 = ROW * 3.0 + 2.0 + GROUP_NAME;
const LABEL: f32 = 12.5;
const GROUP_PAD: f32 = 6.0;

/// A control as it's placed: which one, and where.
struct Placed {
    index: usize,
    rect: Rect,
    big: bool,
    named: bool,
}

/// Where a tab's controls go, starting at the left of `left`, each group as fully as it's given room for, and
/// where each group starts and ends.
fn lay_out(painter: &egui::Painter, groups: &[Group], fits: &[Fit], left: f32, top: f32) -> (Vec<Vec<Placed>>, Vec<(f32, f32)>) {
    let measure = |text: &str| painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(LABEL), egui::Color32::WHITE).size().x;
    let mut x = left;
    let mut placed = Vec::new();
    let mut spans = Vec::new();
    for (group, &fit) in groups.iter().zip(fits) {
        let start = x;
        x += GROUP_PAD;
        let mut mine = Vec::new();
        let mut column: Vec<(usize, f32)> = Vec::new();
        let flush = |column: &mut Vec<(usize, f32)>, x: &mut f32, mine: &mut Vec<Placed>| {
            if column.is_empty() {
                return;
            }
            let width = column.iter().map(|(_, w)| *w).fold(0.0, f32::max);
            for (row, (index, _)) in column.iter().enumerate() {
                mine.push(Placed { index: *index, rect: Rect::from_min_size(pos2(*x, top + row as f32 * ROW), vec2(width, ROW)), big: false, named: fit != Fit::Pictures });
            }
            *x += width + 2.0;
            column.clear();
        };
        for (index, control) in group.controls.iter().enumerate() {
            let label = crate::i18n::tr(control.label);
            let drops = matches!(control.what, What::Drop(_));
            if control.big && fit == Fit::Full {
                flush(&mut column, &mut x, &mut mine);
                let (first, second) = two_lines(label);
                let words = measure(first).max(measure(second)) + if drops && second.is_empty() { 12.0 } else { 0.0 };
                let width = words.max(BIG_ICON + 12.0) + 12.0;
                mine.push(Placed { index, rect: Rect::from_min_size(pos2(x, top), vec2(width, ROW * 3.0)), big: true, named: true });
                x += width + 2.0;
                continue;
            }
            let width = if fit == Fit::Pictures { SMALL_ICON + 12.0 + if drops { 10.0 } else { 0.0 } } else { SMALL_ICON + 14.0 + measure(label) + if drops { 14.0 } else { 0.0 } };
            column.push((index, width));
            if column.len() == 3 {
                flush(&mut column, &mut x, &mut mine);
            }
        }
        flush(&mut column, &mut x, &mut mine);
        x = x.max(start + measure(crate::i18n::tr(group.name)) + GROUP_PAD * 2.0) + GROUP_PAD;
        spans.push((start, x));
        placed.push(mine);
        x += 1.0;
    }
    (placed, spans)
}

/// How much room each group's given to fit the width there is: in full, until there isn't room, when the groups
/// are made smaller from the right, as Office does, first their large buttons and then their names.
fn fit_groups(painter: &egui::Painter, groups: &[Group], available: f32) -> Vec<Fit> {
    let mut fits = vec![Fit::Full; groups.len()];
    let width = |fits: &[Fit]| lay_out(painter, groups, fits, 0.0, 0.0).1.last().map_or(0.0, |(_, end)| *end);
    // First the large buttons made small, a group at a time from the right; then the names left off likewise.
    for step in [Fit::Compact, Fit::Pictures] {
        for g in (0..groups.len()).rev() {
            if width(&fits) <= available {
                return fits;
            }
            if step == Fit::Pictures || groups[g].controls.iter().any(|c| c.big) {
                fits[g] = step;
            }
        }
    }
    fits
}

/// A large button's name, in up to two lines, broken at the space nearest its middle when it's long.
fn two_lines(label: &str) -> (&str, &str) {
    if label.chars().count() <= 12 {
        return (label, "");
    }
    let middle = label.len() / 2;
    let best = label.match_indices(' ').map(|(at, _)| at).min_by_key(|at| at.abs_diff(middle));
    best.map_or((label, ""), |at| (&label[..at], &label[at + 1..]))
}

/// Letters for Alt to show on each of a tab's controls, as Office gives them: each control's own initial if it's
/// free, then another letter of its name, then a number. None where there's nothing left.
pub(super) fn key_letters(labels: &[String]) -> Vec<Option<char>> {
    let mut used: Vec<char> = vec![FILE_KEY];
    labels
        .iter()
        .map(|label| {
            let initials = label.split_whitespace().filter_map(|word| word.chars().next());
            let letters = label.chars();
            // Then any letter left, and the digits, so a crowded tab still has one for each.
            let wanted = initials.chain(letters).chain('A'..='Z').chain('1'..='9').chain(['0']).map(|c| c.to_ascii_uppercase()).filter(char::is_ascii_alphanumeric);
            let found = wanted.into_iter().find(|c| !used.contains(c));
            if let Some(c) = found {
                used.push(c);
            }
            found
        })
        .collect()
}

/// The letter a key gives, for Alt's letters.
fn letter_of(key: egui::Key) -> Option<char> {
    let name = key.name();
    let mut chars = name.chars();
    let first = chars.next()?;
    (chars.next().is_none() && first.is_ascii_alphanumeric()).then_some(first.to_ascii_uppercase())
}

/// Puts a letter in a small box at a point, for Alt's letters: over everything, so the buttons below a tab can't
/// hide its letter.
fn key_tip(ctx: &Context, at: Pos2, letter: char) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::unique("ribbon-key-tips")));
    let rect = Rect::from_center_size(at, vec2(16.0, 16.0));
    painter.rect(rect, 3.0, theme::field(), Stroke::new(1.0, theme::accent()), egui::StrokeKind::Inside);
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, letter.to_string(), egui::FontId::proportional(11.5), theme::strong_text());
}

/// What's shown on hover over a control: its name, its keys, and where it is in the menus.
fn hover_words(control: &Control, found: &[menus::Found]) -> (String, String) {
    let command = match control.what {
        What::Run(command) => Some(command),
        What::Tick(flag) => Some(Command::Toggle(flag)),
        What::Drop(_) => None,
    };
    let entry = command.and_then(|command| found.iter().find(|f| f.command == command && !f.shortcut.is_empty()).or_else(|| found.iter().find(|f| f.command == command)));
    let keys = entry.map_or_else(String::new, |f| menus::shown_shortcut(f.shortcut));
    let place = entry.map_or_else(String::new, |f| f.path.clone());
    (keys, place)
}

impl Limoniter {
    /// The kind of file in front, if it brings a tab of its own.
    pub(super) fn ribbon_kind(&self) -> Option<Kind> {
        if self.no_file() {
            return None;
        }
        let doc = &self.docs[self.active];
        if doc.hex_mode {
            Some(Kind::Hex)
        } else if doc.picture.is_some() {
            Some(Kind::Picture)
        } else if doc.table.is_some() {
            Some(Kind::Table)
        } else if doc.legacy.is_some() {
            Some(Kind::Document)
        } else if self.is_spectrum_basic(self.active) || doc.listing_of.is_some() || preview::kind_for_file(doc.path.as_deref(), "", &doc.text.format) == Some(preview::Kind::Basic) {
            Some(Kind::Basic)
        } else if self.doc_cpu(self.active).is_some() {
            Some(Kind::Assembly)
        } else if doc.text.markdown {
            Some(Kind::Markdown)
        } else {
            None
        }
    }

    /// The ribbon's tabs as they are now: the usual eight, and one for the kind of file in front.
    pub(super) fn ribbon_tabs(&self) -> Vec<Tab> {
        let mut tabs = Tab::FIXED.to_vec();
        tabs.extend(self.ribbon_kind().map(Tab::Kind));
        tabs
    }

    /// The tab shown: the one chosen, unless it was a file's own and that file's no longer in front.
    pub(super) fn ribbon_tab(&self) -> Tab {
        match self.ribbon.tab {
            Tab::Kind(kind) if self.ribbon_kind() != Some(kind) => Tab::Home,
            tab => tab,
        }
    }

    pub(super) fn fold_ribbon(&mut self) {
        if self.view.ribbon {
            self.view.ribbon_folded = !self.view.ribbon_folded;
            self.ribbon.popped = false;
        }
    }

    /// Alt pressed and let go with nothing else shows the letters on the tabs, as Office does; while they're shown,
    /// a letter goes to its tab, then to its button, and Escape goes back. Taken before anything else sees the keys.
    pub(super) fn ribbon_keys(&mut self, ctx: &Context) {
        if !self.view.ribbon {
            self.ribbon.tips = Tips::Off;
            return;
        }
        // Anything else with Alt makes it a shortcut, a click, or Alt+Tab to another window, rather than a tap.
        let (alt, others, clicked) = ctx.input(|i| {
            let pressed = i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. } | egui::Event::Text(_) | egui::Event::WindowFocused(_)));
            (i.modifiers.alt, pressed || i.modifiers.ctrl || i.modifiers.shift || i.modifiers.command || i.pointer.any_pressed(), i.pointer.any_pressed())
        });
        if alt && !self.ribbon.alt_down {
            self.ribbon.alt_down = true;
            self.ribbon.alt_alone = true;
        }
        // Losing the window lets go of every key at once, so it's checked whether Alt is still held or not.
        if (alt || self.ribbon.alt_down) && others {
            self.ribbon.alt_alone = false;
        }
        if !alt && self.ribbon.alt_down {
            self.ribbon.alt_down = false;
            if self.ribbon.alt_alone && matches!(self.dialog, Dialog::None) {
                self.ribbon.tips = if self.ribbon.tips == Tips::Off { Tips::Tabs } else { Tips::Off };
                if self.ribbon.tips == Tips::Off && self.view.ribbon_folded {
                    self.ribbon.popped = false;
                }
            }
        }
        if clicked && self.ribbon.tips != Tips::Off {
            self.ribbon.tips = Tips::Off;
        }
        if self.ribbon.tips == Tips::Off || alt {
            return;
        }
        // The keys pressed, taken so that the text and the shortcuts never see them.
        let pressed: Vec<egui::Key> = ctx.input_mut(|i| {
            let keys = i.events.iter().filter_map(|e| if let egui::Event::Key { key, pressed: true, .. } = e { Some(*key) } else { None }).collect();
            i.events.retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
            keys
        });
        for key in pressed {
            self.ribbon_tip_key(ctx, key);
        }
    }

    /// One key pressed while Alt's letters are shown.
    fn ribbon_tip_key(&mut self, ctx: &Context, key: egui::Key) {
        if key == egui::Key::Escape {
            self.ribbon.tips = if self.ribbon.tips == Tips::Buttons { Tips::Tabs } else { Tips::Off };
            if self.ribbon.tips == Tips::Off {
                self.ribbon.popped = false;
            }
            return;
        }
        let Some(letter) = letter_of(key) else { return };
        match self.ribbon.tips {
            Tips::Off => {}
            Tips::Tabs => {
                if letter == FILE_KEY {
                    self.ribbon.tips = Tips::Off;
                    if let Some(id) = self.ribbon.file_button {
                        Popup::open_id(ctx, id.with("popup"));
                    }
                    return;
                }
                if let Some(tab) = self.ribbon_tabs().into_iter().find(|tab| tab.key() == letter) {
                    self.ribbon.tab = tab;
                    self.ribbon.tips = Tips::Buttons;
                    if self.view.ribbon_folded {
                        self.ribbon.popped = true;
                    }
                }
            }
            Tips::Buttons => {
                let tab = self.ribbon_tab();
                let controls: Vec<(usize, usize, &Control)> = tab.groups().iter().enumerate().flat_map(|(g, group)| group.controls.iter().enumerate().map(move |(c, control)| (g, c, control))).collect();
                let labels: Vec<String> = controls.iter().map(|(_, _, control)| crate::i18n::tr(control.label).to_string()).collect();
                let letters = key_letters(&labels);
                let Some(found) = letters.iter().position(|l| *l == Some(letter)) else { return };
                let (g, c, control) = controls[found];
                self.ribbon.tips = Tips::Off;
                match control.what {
                    What::Run(command) if self.can(command) => {
                        self.pending_command = Some(command);
                        self.ribbon.popped = false;
                    }
                    What::Tick(flag) if self.can(Command::Toggle(flag)) => {
                        self.pending_command = Some(Command::Toggle(flag));
                        self.ribbon.popped = false;
                    }
                    What::Drop(_) => Popup::open_id(ctx, control_id(tab, g, c).with("popup")),
                    _ => {}
                }
            }
        }
    }

    /// The bar along the top when the ribbon's on: the File button, then the tabs. The menus' place.
    pub(super) fn ribbon_tab_bar(&mut self, ui: &mut Ui, chosen: &mut Option<Command>) {
        let tips = self.ribbon.tips == Tips::Tabs;
        // The File button, as Office has, holding the File menu with the Window and Help menus after it.
        let file = egui::Button::new(RichText::new(crate::i18n::tr("File")).color(theme::accent())).frame(false);
        let menu = ui.add(file);
        self.ribbon.file_button = Some(menu.id);
        ui.ctx().data_mut(|d| d.insert_temp(super::access_host::first_menu_key(), menu.id));
        if tips {
            key_tip(ui.ctx(), menu.rect.left_bottom() + vec2(10.0, 0.0), FILE_KEY);
        }
        crate::fit::popup_menu(&menu, |ui| {
            let file = self.drop_down_list(ui, menus::FILE);
            let more = self.drop_down_list(ui, FILE_BUTTON);
            *chosen = chosen.or(file).or(more);
        });
        ui.add_space(6.0);
        let shown = self.ribbon_tab();
        for tab in self.ribbon_tabs() {
            let name = crate::i18n::tr(tab.name());
            let galley = ui.painter().layout_no_wrap(name.to_string(), egui::FontId::proportional(13.5), theme::text());
            let size = vec2(galley.size().x + 22.0, ui.spacing().interact_size.y.max(24.0));
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            let on = tab == shown && (!self.view.ribbon_folded || self.ribbon.popped);
            let own = matches!(tab, Tab::Kind(_));
            if on {
                ui.painter().rect_filled(rect, egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, theme::window());
            } else if response.hovered() {
                ui.painter().rect_filled(rect.shrink(1.0), 5.0, theme::hover());
            }
            if own {
                // A file's own tab stands out in the accent colour, as Office's picture and table tabs do.
                ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(rect.width(), 2.5)), 1.0, theme::accent());
            }
            let ink = if own { theme::accent() } else if on { theme::strong_text() } else { theme::text() };
            ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink);
            if response.has_focus() {
                ui.painter().rect_stroke(rect.shrink(1.0), 4.0, Stroke::new(1.5, theme::accent()), egui::StrokeKind::Inside);
            }
            if tips {
                key_tip(ui.ctx(), pos2(rect.center().x, rect.bottom()), tab.key());
            }
            crate::access::chosen(&response, egui::Role::Tab, tab == shown, name);
            let response = response.on_hover_text(if self.view.ribbon_folded { crate::i18n::tr("Click to show this tab's buttons; double-click, or Ctrl+F1, to keep the ribbon open") } else { crate::i18n::tr("Double-click, or Ctrl+F1, to fold the ribbon to its tabs") });
            if response.double_clicked() {
                *chosen = Some(Command::FoldRibbon);
                self.ribbon.tab = tab;
            } else if response.clicked() {
                if self.view.ribbon_folded {
                    self.ribbon.popped = !(self.ribbon.popped && tab == shown);
                }
                self.ribbon.tab = tab;
            }
        }
        self.ribbon.bar_bottom = ui.min_rect().max.y;
    }

    /// The ribbon's buttons, under the bar: in place when it's open, over the text when it's folded and a tab's
    /// clicked. Returns the command chosen.
    pub(super) fn ribbon_body(&mut self, ui: &mut Ui) -> Option<Command> {
        if !self.view.ribbon {
            return None;
        }
        let ctx = ui.ctx().clone();
        if !self.view.ribbon_folded {
            self.ribbon.popped = false;
            let mut chosen = None;
            egui::Panel::top("ribbon-body").show_separator_line(true).frame(egui::Frame::new().fill(theme::window()).inner_margin(egui::Margin { left: 8, right: 8, top: 4, bottom: 3 })).show(ui, |ui| {
                chosen = self.ribbon_buttons(ui);
            });
            return chosen;
        }
        if !self.ribbon.popped {
            return None;
        }
        // Folded: the tab's buttons over the text, until a click elsewhere or Escape puts them away.
        let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|at| self.ribbon.popped_rect.is_some_and(|r| !r.contains(at)) && at.y > self.ribbon.bar_bottom));
        if outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.ribbon.tips == Tips::Off {
            self.ribbon.popped = false;
            return None;
        }
        let width = ctx.content_rect().width() - 16.0;
        let mut chosen = None;
        let shown = egui::Area::new(Id::unique("ribbon-popped")).default_width(width).order(egui::Order::Foreground).fixed_pos(pos2(8.0, self.ribbon.bar_bottom + 2.0)).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(theme::window()).show(ui, |ui| {
                ui.set_width(width - 16.0);
                chosen = self.ribbon_buttons(ui);
            });
        });
        self.ribbon.popped_rect = Some(shown.response.rect);
        if chosen.is_some() {
            self.ribbon.popped = false;
        }
        chosen
    }

    /// The groups of the tab shown, laid out to fit: in full if there's room, else smaller, else scrolled.
    fn ribbon_buttons(&mut self, ui: &mut Ui) -> Option<Command> {
        let tab = self.ribbon_tab();
        let groups = tab.groups();
        let available = ui.available_width();
        let painter = ui.painter().clone();
        let fits = fit_groups(&painter, groups, available);
        let found = menus::every_command();
        let mut chosen = None;
        let width = lay_out(&painter, groups, &fits, 0.0, 0.0).1.last().map_or(0.0, |(_, end)| *end);
        if width <= available {
            chosen = self.draw_groups(ui, tab, &fits, &found);
        } else {
            // Even as pictures alone it's too wide: it scrolls sideways.
            egui::ScrollArea::horizontal().id_salt("ribbon-scroll").show(ui, |ui| chosen = self.draw_groups(ui, tab, &fits, &found));
        }
        chosen
    }

    fn draw_groups(&mut self, ui: &mut Ui, tab: Tab, fits: &[Fit], found: &[menus::Found]) -> Option<Command> {
        let groups = tab.groups();
        let origin = ui.cursor().min;
        let (placed, spans) = lay_out(ui.painter(), groups, fits, origin.x, origin.y);
        let total = spans.last().map_or(0.0, |(_, end)| end - origin.x);
        ui.allocate_exact_size(vec2(total, BODY), Sense::hover());
        let tips = self.ribbon.tips == Tips::Buttons;
        let labels: Vec<String> = groups.iter().flat_map(|g| g.controls.iter()).map(|c| crate::i18n::tr(c.label).to_string()).collect();
        let letters = if tips { key_letters(&labels) } else { Vec::new() };
        let mut chosen = None;
        let mut number = 0;
        for (g, (group, (start, end))) in groups.iter().zip(spans.iter()).enumerate() {
            // The group's name under its buttons, and a line after it.
            let name_at = pos2((start + end) / 2.0, origin.y + BODY - GROUP_NAME / 2.0);
            ui.painter().text(name_at, egui::Align2::CENTER_CENTER, crate::i18n::tr(group.name), egui::FontId::proportional(11.0), theme::text_faint());
            ui.painter().line_segment([pos2(*end, origin.y + 4.0), pos2(*end, origin.y + BODY - 4.0)], Stroke::new(1.0, theme::border()));
            for place in &placed[g] {
                let control = &group.controls[place.index];
                if let Some(command) = self.ribbon_control(ui, tab, g, place, control, found) {
                    chosen = Some(command);
                }
                if tips && let Some(Some(letter)) = letters.get(number + place.index) {
                    let at = if place.big { pos2(place.rect.center().x, place.rect.bottom() - 4.0) } else { pos2(place.rect.min.x + 10.0, place.rect.bottom() - 2.0) };
                    key_tip(ui.ctx(), at, *letter);
                }
            }
            number += group.controls.len();
        }
        chosen
    }

    /// One control, drawn as it is now and asked of exactly as its menu item is.
    fn ribbon_control(&mut self, ui: &mut Ui, tab: Tab, group: usize, place: &Placed, control: &Control, found: &[menus::Found]) -> Option<Command> {
        let (enabled, ticked) = match control.what {
            What::Run(command) => (self.can(command), None),
            What::Tick(flag) => (self.can(Command::Toggle(flag)), Some(self.flag_on(flag))),
            What::Drop(_) => (true, None),
        };
        #[cfg(test)]
        match control.what {
            What::Run(command) => note_drawn(ui.ctx(), true, command, enabled, None),
            What::Tick(flag) => note_drawn(ui.ctx(), true, Command::Toggle(flag), enabled, ticked),
            What::Drop(_) => {}
        }
        let id = control_id(tab, group, place.index);
        let rect = place.rect.shrink2(vec2(1.0, 1.0));
        let response = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
        let label = crate::i18n::tr(control.label);
        let painter = ui.painter();
        let on = ticked == Some(true);
        let open = matches!(control.what, What::Drop(_)) && Popup::is_id_open(ui.ctx(), Popup::default_response_id(&response));
        if on || open {
            painter.rect(rect, 4.0, theme::selected(), Stroke::new(1.0, theme::accent().gamma_multiply(0.6)), egui::StrokeKind::Inside);
        } else if enabled && response.hovered() {
            painter.rect_filled(rect, 4.0, theme::hover());
        }
        if response.has_focus() {
            painter.rect_stroke(rect, 4.0, Stroke::new(1.5, theme::accent()), egui::StrokeKind::Inside);
        }
        let ink = if enabled { theme::text() } else { theme::text_disabled() };
        let accent = if enabled { theme::accent() } else { theme::text_disabled() };
        let drops = matches!(control.what, What::Drop(_));
        if place.big {
            let icon = Rect::from_center_size(pos2(rect.center().x, rect.min.y + 6.0 + BIG_ICON / 2.0), vec2(BIG_ICON, BIG_ICON));
            ribbon_icons::paint(painter, icon, control.icon, ink, accent, theme::window());
            let (first, second) = two_lines(label);
            let font = egui::FontId::proportional(LABEL);
            let y = icon.max.y + 4.0 + LABEL / 2.0 + 1.0;
            let first_galley = painter.layout_no_wrap(first.to_string(), font.clone(), ink);
            let first_width = first_galley.size().x;
            let shift = if drops && second.is_empty() { 6.0 } else { 0.0 };
            painter.galley(pos2(rect.center().x - first_width / 2.0 - shift, y - first_galley.size().y / 2.0), first_galley, ink);
            if !second.is_empty() {
                painter.text(pos2(rect.center().x, y + LABEL + 2.0), egui::Align2::CENTER_CENTER, second, font, ink);
            }
            if drops {
                let at = if second.is_empty() { pos2(rect.center().x + first_width / 2.0 + 2.0, y) } else { pos2(rect.center().x, y + LABEL * 2.0 + 2.0) };
                ribbon_icons::chevron(painter, at, 6.0, ink);
            }
        } else {
            let icon = Rect::from_center_size(pos2(rect.min.x + 4.0 + SMALL_ICON / 2.0, rect.center().y), vec2(SMALL_ICON, SMALL_ICON));
            ribbon_icons::paint(painter, icon, control.icon, ink, accent, theme::window());
            let mut end = icon.max.x;
            if place.named {
                end = painter.text(pos2(icon.max.x + 6.0, rect.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(LABEL), ink).max.x;
            }
            if drops {
                // Just after the name, as a menu's arrow follows its words.
                ribbon_icons::chevron(painter, pos2(end + 8.0, rect.center().y), 6.0, ink);
            }
        }
        // Read out as the menu item it is: a button, or a check box with its tick, greyed when it can't act.
        match control.what {
            What::Tick(_) => response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, on, label)),
            _ => response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label)),
        }
        let (keys, where_in) = hover_words(control, found);
        let mut said = keys.clone();
        if !where_in.is_empty() {
            if !said.is_empty() {
                said += ", ";
            }
            said += &crate::i18n::tf!("in {}", where_in.replace(" \u{203A} ", ", "));
        }
        crate::access::describe(&response, &said);
        let hover = |ui: &mut Ui| {
            ui.label(RichText::new(label).strong());
            if !keys.is_empty() {
                ui.label(RichText::new(&keys).color(theme::accent()));
            }
            if !where_in.is_empty() {
                ui.label(RichText::new(crate::i18n::tf!("In the menus: {}", where_in)).color(theme::text_faint()).size(11.5));
            }
        };
        let response = if enabled { response.on_hover_ui(hover) } else { response.on_disabled_hover_ui(hover) };
        match control.what {
            What::Run(command) if response.clicked() => Some(command),
            What::Tick(flag) if response.clicked() => Some(Command::Toggle(flag)),
            What::Drop(entries) => {
                let mut chosen = None;
                // Open recent's tabs change what it lists without closing it.
                if matches!(entries, [Entry::Live(_, Live::RecentFiles)]) {
                    crate::fit::popup_menu_kept_open(&response, |ui| chosen = self.drop_down_list(ui, entries));
                } else {
                    crate::fit::popup_menu(&response, |ui| chosen = self.drop_down_list(ui, entries));
                }
                chosen
            }
            _ => None,
        }
    }

    /// The switch beside Find a command that turns the ribbon on and off: a small ribbon, filled in the accent
    /// colour while it's on. True when it's clicked.
    pub(super) fn ribbon_switch(&self, ui: &mut Ui) -> bool {
        let on = self.view.ribbon;
        let (rect, response) = ui.allocate_exact_size(vec2(32.0, 24.0), Sense::click());
        let fill = if on { theme::accent() } else if response.hovered() { theme::hover() } else { theme::field() };
        ui.painter().rect(rect, 6.0, fill, Stroke::new(1.0, if on { theme::accent() } else { theme::border() }), egui::StrokeKind::Inside);
        let (ink, accent, back) = if on { (theme::window(), theme::window(), theme::accent()) } else { (theme::text_dim(), theme::accent(), theme::field()) };
        ribbon_icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(18.0, 18.0)), Icon::Ribbon, ink, accent, back);
        if response.has_focus() {
            ui.painter().rect_stroke(rect.expand(1.0), 7.0, Stroke::new(1.5, theme::strong_text()), egui::StrokeKind::Outside);
        }
        crate::access::chosen(&response, egui::Role::Switch, on, crate::i18n::tr("Ribbon"));
        let words = if on { crate::i18n::tr("Ribbon: on. Click for the menus instead") } else { crate::i18n::tr("Ribbon: off. Click for tabs of buttons with pictures, in place of the menus") };
        crate::access::describe(&response, words);
        response.on_hover_text(words).clicked()
    }
}

/// A control's id, the same each frame, so Alt's letters can open its list.
fn control_id(tab: Tab, group: usize, index: usize) -> Id {
    Id::unique(("ribbon-control", tab.name(), group, index))
}

/// What a test sees of each control drawn: where from (the ribbon or a menu), its command, and its state.
#[cfg(test)]
#[derive(Clone, Copy)]
pub(super) struct Drawn {
    pub ribbon: bool,
    pub command: Command,
    pub enabled: bool,
    pub ticked: Option<bool>,
}

#[cfg(test)]
pub(super) fn drawn_key() -> Id {
    Id::unique("drawn-controls-for-tests")
}

/// Notes a control as it's drawn, for the test that checks the ribbon and the menus agree.
#[cfg(test)]
pub(super) fn note_drawn(ctx: &Context, ribbon: bool, command: Command, enabled: bool, ticked: Option<bool>) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<Drawn>>(drawn_key()).push(Drawn { ribbon, command, enabled, ticked }));
}

#[cfg(test)]
#[path = "ribbon_tests.rs"]
mod tests;
