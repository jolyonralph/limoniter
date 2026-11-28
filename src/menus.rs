//! The menu bar, laid out by what things are for, and a box that finds any command by its name (F1).
//!
//! Everything reachable from the menus is set out in the tables below, once. The menus draw from them, the command
//! box searches them, and a test checks that nothing is left out.

use super::*;
use crate::conversions::Conversion;
use crate::preview;
use super::start_page::Guide;
use super::php_host::PhpAction;
use super::python_host::PyAction;
use super::notebook_host::Act as NotebookAct;
use super::database_host::Act as DatabaseAct;
use crate::text_objects::{Action as TextAction, Object as TextObject};
use crate::app::words_host::{Action as WordsAction, Switch as WordsSwitch};

/// A switch that's simply on or off, toggled from a menu or the command box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Flag {
    LineNumbers,
    Ruler,
    ColumnMarkers,
    SpacesForTab,
    DocumentMap,
    Breadcrumbs,
    StickyScroll,
    IndentGuides,
    HideComments,
    PanelsHide,
    /// Project and Functions in one strip at the left.
    StackPanels,
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
    /// The open files as a list down the side, in place of the row of tabs.
    SideTabs,
    /// That list on the right rather than the left.
    SideTabsRight,
    /// The row of buttons under the menus.
    Toolbar,
    StatusBar,
    OutputWindow,
    WordWrap,
    ReadOnly,
    ColumnMode,
    AlwaysOnTop,
    FullScreen,
    Spelling,
    GitBlame,
    CycleCounts,
    AssistantPanel,
    Ribbon,
    /// php -l on each PHP file saved.
    PhpLintOnSave,
    Panel(PanelFlag),
    /// The marks of the Words tools: syllable counts, repeated words, filler words and adverbs.
    Words(crate::app::words_host::Switch),
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
    Todos,
    FilesChanging,
    Agents,
    FileProperties,
    Json,
    Xml,
    Characters,
    EncodingPreview,
    Clipboard,
    Strings,
    Inspector,
    Structure,
    CallGraph,
    Instructions,
    Variables,
    Database,
    /// The Python and R console, kept by `python_host`.
    Console,
    Tests,
    Containers,
}

/// Lists that change, drawn as they are now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Live {
    ClaudeCode,
    WindowThemes,
    DocumentThemes,
    ThisDocumentTheme,
    Devices,
    Machines,
    RecentFiles,
    NewAs,
    Tasks,
    RecentProjects,
    ProjectAutosave,
    Macros,
    Scripts,
    AddOns,
    TabSize,
    Snippets,
    Tags,
    /// The colour tags to choose from, by name.
    TagList,
    Bookmarks,
    SurroundSnippets,
    Layouts,
    DeleteLayouts,
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

// Sub-menus kept by name, so the ribbon's drop-down lists are the very same lists as the menus'.
pub(super) const CLIPBOARDS: &[Entry] = &[
            Keyed("The computer's clipboard", Command::UseClipboard(0), "Ctrl+0"),
            Line,
            Keyed("Clipboard 1", Command::UseClipboard(1), "Ctrl+1"),
            Keyed("Clipboard 2", Command::UseClipboard(2), "Ctrl+2"),
            Keyed("Clipboard 3", Command::UseClipboard(3), "Ctrl+3"),
            Keyed("Clipboard 4", Command::UseClipboard(4), "Ctrl+4"),
            Keyed("Clipboard 5", Command::UseClipboard(5), "Ctrl+5"),
            Keyed("Clipboard 6", Command::UseClipboard(6), "Ctrl+6"),
            Keyed("Clipboard 7", Command::UseClipboard(7), "Ctrl+7"),
            Keyed("Clipboard 8", Command::UseClipboard(8), "Ctrl+8"),
            Keyed("Clipboard 9", Command::UseClipboard(9), "Ctrl+9"),
        ];

pub(super) const SELECT: &[Entry] = &[
            Sub("Inside", &SELECT_INSIDE),
            Sub("Around", &SELECT_AROUND),
            Line,
            Item("To the matching bracket", Command::SelectToBrace),
            Item("Every copy of the selection", Command::MakeSelections),
        ];

pub(super) const LINES: &[Entry] = &[
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
        ];

pub(super) const COLUMNS: &[Entry] = &[
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
        ];

pub(super) const CHANGE_CASE: &[Entry] = &[
            Keyed("UPPER CASE", Command::Upper, "Alt+U"),
            Keyed("lower case", Command::Lower, "Alt+L"),
            Item("Sentence case", Command::SentenceCase),
            Item("Title Case", Command::TitleCase),
            Line,
            Item("ROT13", Command::Rot13),
            Item("ROT26", Command::Rot26),
        ];

pub(super) const INSERT: &[Entry] = &[
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
        ];

pub(super) const SURROUND: &[Entry] = &[
            Item("( and )", Command::Surround('(', ')')),
            Item("[ and ]", Command::Surround('[', ']')),
            Item("{ and }", Command::Surround('{', '}')),
            Item("< and >", Command::Surround('<', '>')),
            Line,
            Item("\" and \"", Command::Surround('"', '"')),
            Item("' and '", Command::Surround('\'', '\'')),
            Item("` and `", Command::Surround('`', '`')),
            Item("\u{201C} and \u{201D}", Command::Surround('\u{201C}', '\u{201D}')),
            Line,
            Item("A tag\u{2026}", Command::SurroundTag),
            Item("An Emmet abbreviation\u{2026}", Command::WrapAbbreviation),
            Entry::Live("A snippet", Live::SurroundSnippets),
        ];

pub(super) const COLOUR_TAG: &[Entry] = &[
            Entry::Live("", Live::TagList),
            Line,
            Item("New tag\u{2026}", Command::NewTag),
            Item("Edit tags\u{2026}", Command::EditTags),
            Line,
            Keyed("Remove the tag", Command::Untag, "Ctrl+Shift+0"),
        ];

pub(super) const BOOKMARKS: &[Entry] = &[
            Keyed("Set or clear", Command::ToggleBookmark, "Ctrl+F2"),
            Keyed("Next", Command::NextBookmark, "F2"),
            Keyed("Previous", Command::PreviousBookmark, "Shift+F2"),
            Entry::Live("Go to", Live::Bookmarks),
            Line,
            Item("Copy the bookmarked lines", Command::CopyBookmarked),
            Item("Cut the bookmarked lines", Command::CutBookmarked),
            Item("Delete the bookmarked lines", Command::DeleteBookmarked),
            Item("Delete the lines that aren't bookmarked", Command::DeleteUnbookmarked),
            Line,
            Item("Swap bookmarked and not", Command::InvertBookmarks),
            Item("Clear every bookmark", Command::ClearBookmarks),
            Check("Bookmarks panel", Flag::Panel(PanelFlag::Bookmarks)),
        ];

pub(super) const THEME: &[Entry] = &[
            Sub("Window", &[Entry::Live("Window themes", Live::WindowThemes)]),
            Sub("Documents", &[Entry::Live("Document themes", Live::DocumentThemes)]),
            Sub("This document only", &[Entry::Live("This document's theme", Live::ThisDocumentTheme)]),
            Line,
            Item("Themes and rules\u{2026}", Command::Themes),
        ];

pub(super) const SHOW: &[Entry] = &[
            Check("Line numbers", Flag::LineNumbers),
            Check("Ruler", Flag::Ruler),
            Check("Column markers", Flag::ColumnMarkers),
            Check("Document map", Flag::DocumentMap),
            Check("Breadcrumbs", Flag::Breadcrumbs),
            Check("Indentation guides", Flag::IndentGuides),
            Check("Hide comments", Flag::HideComments),
            Check("Panels hide themselves", Flag::PanelsHide),
            Check("Sticky scroll", Flag::StickyScroll),
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
        ];

pub(super) const FOLDING: &[Entry] = &[Item("Fold everything", Command::CollapseAll), Item("Unfold everything", Command::ExpandAll), Line, Item("Hide the selected lines", Command::HideLines), Item("Show the selected lines", Command::ShowLines)];

pub(super) const PREVIEW: &[Entry] = &[Item("Off", Command::SetPreview(settings::PreviewView::Text)), Item("Beside the text", Command::SetPreview(settings::PreviewView::Split)), Item("Instead of the text", Command::SetPreview(settings::PreviewView::Full))];

pub(super) const HEX: &[Entry] = &[
            Item("As text", Command::ViewRegular),
            Keyed("As hex", Command::ViewHex, "Ctrl+H"),
            Item("As hex, in EBCDIC", Command::ViewEbcdic),
            Line,
            Item("Insert or delete bytes\u{2026}", Command::HexInsertDelete),
            Item("Copy the selected rows", Command::HexCopySelected),
            Line,
            Check("String explorer", Flag::Panel(PanelFlag::Strings)),
            Check("Value inspector", Flag::Panel(PanelFlag::Inspector)),
            Check("Structure", Flag::Panel(PanelFlag::Structure)),
        ];

pub(super) const SPLIT: &[Entry] = &[Item("One above the other", Command::SplitHorizontal), Item("Side by side", Command::SplitVertical), Item("Another tab on this file", Command::DuplicateWindow), Keyed("Another file beside this one", Command::OpenBeside, "Ctrl+\\")];

pub(super) const PANELS: &[Entry] = &[
            Check("Files on this computer", Flag::Panel(PanelFlag::FileExplorer)),
            Check("Project", Flag::Panel(PanelFlag::Project)),
            Check("Open files", Flag::Panel(PanelFlag::OpenFiles)),
            Check("File lists", Flag::Panel(PanelFlag::FileLists)),
            Check("Database", Flag::Panel(PanelFlag::Database)),
            Check("Tests", Flag::Panel(PanelFlag::Tests)),
            Check("Containers", Flag::Panel(PanelFlag::Containers)),
            Line,
            Check("Functions", Flag::Panel(PanelFlag::Functions)),
            Check("Stack Project and Functions", Flag::StackPanels),
            Check("Call graph", Flag::Panel(PanelFlag::CallGraph)),
            Check("Instruction reference", Flag::Panel(PanelFlag::Instructions)),
            Check("Bookmarks", Flag::Panel(PanelFlag::Bookmarks)),
            Check("TODO notes", Flag::Panel(PanelFlag::Todos)),
            Check("Files changing", Flag::Panel(PanelFlag::FilesChanging)),
            Check("AI agents", Flag::Panel(PanelFlag::Agents)),
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
            Check("Value inspector", Flag::Panel(PanelFlag::Inspector)),
            Check("Structure", Flag::Panel(PanelFlag::Structure)),
            Check("Variables", Flag::Panel(PanelFlag::Variables)),
            Check("Python and R console", Flag::Panel(PanelFlag::Console)),
            Check("Clipboard history", Flag::Panel(PanelFlag::Clipboard)),
            Line,
            Item("Close every panel", Command::HidePanels),
        ];

pub(super) const ENCODING: &[Entry] = &[
            Item("Convert to\u{2026}", Command::ConvertTo),
            Item("Reopen with\u{2026}", Command::ReopenAs),
            Item("Save a copy in\u{2026}", Command::SaveCopyAs),
            Item("Kind of computer for new files\u{2026}", Command::ChoosePreset),
        ];

/// The tokenised BASIC programs a listing can be saved as: in File, Export, and the ribbon's drop-down.
pub(super) const TOKENISED_BASIC: &[Entry] = &[
            Item("Save as a Spectrum tape (.tap)\u{2026}", Command::SaveSpectrumTape),
            Item("Save as a Spectrum Next program (.bas)\u{2026}", Command::SaveNextProgram),
            Item("Save as a Commodore program (.prg)\u{2026}", Command::SaveCommodoreProgram),
            Item("Save as a BBC BASIC program\u{2026}", Command::SaveBbcProgram),
            Item("Save as a BBC BASIC (Z80) program, for the Z88 or NC100\u{2026}", Command::SaveBbcZ80Program),
            Item("Save as an AMOS program (.AMOS)\u{2026}", Command::SaveAmosProgram),
            Item("Save as a STOS program (.BAS)\u{2026}", Command::SaveStosProgram),
        ];

pub(super) const SAVE_DOCUMENT_AS: &[Entry] = &[
            Item("Plain text\u{2026}", Command::SaveDocumentAs(crate::legacy_docs::Target::Text)),
            Item("Markdown\u{2026}", Command::SaveDocumentAs(crate::legacy_docs::Target::Markdown)),
            Item("RTF\u{2026}", Command::SaveDocumentAs(crate::legacy_docs::Target::Rtf)),
            Item("CSV, for a spreadsheet\u{2026}", Command::SaveDocumentAs(crate::legacy_docs::Target::Csv)),
        ];

pub(super) const TABS_AND_SPACES: &[Entry] = &[
            Check("Tab key types spaces", Flag::SpacesForTab),
            Entry::Live("Tab size", Live::TabSize),
            Line,
            Item("Tabs to spaces", Command::TabsToSpaces),
            Item("Tabs to spaces, in the indent only", Command::LeadingTabsToSpaces),
            Item("Spaces to tabs, in the indent", Command::SpacesToTabs),
            Line,
            Item("Trim spaces at the ends of lines", Command::TrimTrailing),
            Item("Trim spaces at the starts of lines", Command::TrimLeading),
            Line,
            Check("Show spaces and tabs", Flag::Whitespace),
        ];

pub(super) const PARAGRAPH: &[Entry] = &[
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
        ];

pub(super) const LAY_OUT_CODE: &[Entry] = &[
            Item("HTML", Command::Format(formatters::Kind::Html)),
            Item("XML", Command::Format(formatters::Kind::Xml)),
            Item("JSON", Command::FormatJson),
            Item("JavaScript", Command::Format(formatters::Kind::Js)),
            Item("CSS", Command::Format(formatters::Kind::Css)),
            Item("SQL", Command::Format(formatters::Kind::Sql)),
            Item("C-like code, brace on the same line", Command::Format(formatters::Kind::CodeAttach)),
            Item("C-like code, brace on its own line", Command::Format(formatters::Kind::CodeBreak)),
            Item("EDI, one segment to a line", Command::Format(formatters::Kind::EdiLines)),
        ];

pub(super) const COMPRESS: &[Entry] = &[
            Item("HTML", Command::Format(formatters::Kind::HtmlCompact)),
            Item("XML", Command::Format(formatters::Kind::XmlCompact)),
            Item("JSON", Command::CompactJson),
            Item("JavaScript", Command::Format(formatters::Kind::JsCompact)),
            Item("CSS", Command::Format(formatters::Kind::CssCompact)),
            Item("EDI, onto one line", Command::Format(formatters::Kind::EdiOneLine)),
        ];

pub(super) const CONVERT: &[Entry] = &[
            Item("Base64 encode", Command::Convert(Conversion::Base64Encode)),
            Item("Base64 decode", Command::Convert(Conversion::Base64Decode)),
            Line,
            Item("JSON to XML", Command::Convert(Conversion::JsonToXml)),
            Item("XML to JSON", Command::Convert(Conversion::XmlToJson)),
        ];

pub(super) const TABLE: &[Entry] = &[Item("Comma-separated to fixed columns\u{2026}", Command::CsvToFixed), Item("Fixed columns to comma-separated\u{2026}", Command::FixedToCsv)];

pub(super) const GENERAL: &[Entry] = &[
            Item("Disassemble\u{2026}", Command::Disassemble),
            Item("Save the decrunched program\u{2026}", Command::SaveDecrunched),
            Item("Character and sprite editor", Command::PixelEditor),
            Item("Tile map and level editor", Command::TileMapEditor),
            Item("Picture converter: Spectrum, Next, CPC, MSX, BBC", Command::RetroPictures),
            Item("Sound effects: AY, SID, 2600, beeper", Command::SoundEffects),
            Item("Ripper: music and graphics in any file", Command::Ripper),
            Item("Table generator (sine, ramps\u{2026})", Command::TableGenerator),
            Item("Raster and copper designer: Amiga, C64, ST", Command::RasterDesigner),
            Item("Checksums and ROM headers", Command::Checksums),
            Item("ROM patches: IPS, BPS, UPS", Command::RomPatches),
            Item("Entropy and byte counts", Command::Entropy),
        ];

pub(super) const HARDWARE: &[Entry] = &[
            Item("Serial terminal and XMODEM/YMODEM\u{2026}", Command::SerialTerminal),
            Item("EPROM programmer: TL866 and T48 (minipro)\u{2026}", Command::EpromProgrammer),
        ];

pub(super) const AMIGA: &[Entry] = &[
            Item("Hardware registers and copper lists", Command::AmigaHardware),
            Item("Bitplane, sprite and bob converter", Command::AmigaBitplanes),
            Item("Blitter minterms", Command::BlitterMinterms),
            Item("NDK (code hints)\u{2026}", Command::AmigaNdk),
            Item("AutoDoc reader", Command::AutoDocs),
            Item("Crunchers and music\u{2026}", Command::AmigaTools),
            Item("Workbench icon\u{2026}", Command::AmigaIcon),
            Item("Boot disk with no file system (trackmo)", Command::AmigaBootDisk),
            Line,
            Item("New Amiga C project\u{2026}", Command::AmigaNewProject),
            Item("Import an Amiga C project\u{2026}", Command::AmigaImportProject),
            Item("Bring the makefiles up to date", Command::AmigaMakefiles),
            Line,
            Item("Check the catalog's translations", Command::CatalogCheck),
            Item("Make the catalogs and the strings' header", Command::CatalogMake),
            Item("Make the catalogs with FlexCat", Command::CatalogFlexCat),
            Item("New translation, or fill one in\u{2026}", Command::CatalogTranslation),
            Line,
            Item("How To Code: writing Amiga demos", Command::HowToCode),
            Item("Copy How To Code's examples to a folder\u{2026}", Command::HowToCodeExamples),
        ];

pub(super) const ATARI_2600: &[Entry] = &[Item("Sprites and playfield", Command::Atari2600Graphics)];

pub(super) const FANTASY: &[Entry] = &[Item("Cart's sprites, map and sounds", Command::CartView)];

pub(super) const ATARI_ST: &[Entry] = &[
            Item("Hardware registers and TOS calls", Command::AtariHardware),
            Item("Picture, sprite and font converter", Command::StPictures),
            Item("STE blitter", Command::AtariBlitter),
            Item("MFP timers", Command::AtariTimers),
            Item("Music, packers and YM2149\u{2026}", Command::StTools),
            Line,
            Item("Profile in Hatari", Command::HatariProfile),
            Item("Make a bootable ST floppy\u{2026}", Command::StFloppy),
        ];

pub(super) const C64: &[Entry] = &[
            Item("VIC-II, SID, CIA and KERNAL guide", Command::C64Hardware),
            Item("Picture, sprite and charset converter", Command::C64Pictures),
            Item("Raster line calculator", Command::C64RasterLines),
            Item("Raster interrupts (stable, chained)", Command::C64RasterIrqs),
            Item("SID music and packers\u{2026}", Command::C64Tools),
            Check("Cycle counts beside 6502, Z80, 6809 and 68000 code", Flag::CycleCounts),
        ];

pub(super) const DRAGON: &[Entry] = &[Item("Dragon DOS disk\u{2026}", Command::DragonDisk)];

/// The first computers, built in for teaching: each one's starters, run with F9 on the machine itself.
pub(super) const EARLY: &[Entry] = &[Item("Manchester Baby (SSEM, 1948): Kilburn's first program", Command::NewUsual("Manchester Baby (SSEM, 1948): Kilburn's first program")), Item("Manchester Baby (SSEM, 1948): a first program", Command::NewUsual("Manchester Baby (SSEM, 1948): a first program")), Item("EDSAC (1949): Wilkes's table of squares", Command::NewUsual("EDSAC (1949): Wilkes's table of squares")), Item("EDSAC (1949): a first program (Initial Orders 2)", Command::NewUsual("EDSAC (1949): a first program (Initial Orders 2)")), Item("PDP-8 (1965): hello on the Teletype (PAL)", Command::NewUsual("PDP-8 (1965): hello on the Teletype (PAL)")), Item("PDP-8 (1965): echo the keyboard (PAL)", Command::NewUsual("PDP-8 (1965): echo the keyboard (PAL)"))];

pub(super) const SPECTRUM: &[Entry] = &[
            // Typing and seeing Spectrum BASIC: they're the Spectrum's, not about turning a listing into a file.
            Keyed("Spectrum GRAPH mode", Command::GraphMode, "Ctrl+Shift+9"),
            Item("Spectrum UDG pictures from\u{2026}", Command::UdgPictures),
            Line,
            Item("Screen designer", Command::ScreenDesigner),
            Item("Next Layer 2 painter", Command::NextPainter),
            Line,
            Item("How To Code: the ZX Spectrum", Command::HowToCodeSpectrum),
            Item("Copy How To Code's Spectrum examples to a folder\u{2026}", Command::HowToCodeSpectrumExamples),
        ];

pub(super) const TASKS: &[Entry] = &[
            Entry::Live("Run", Live::Tasks),
            Line,
            Keyed("Build", Command::BuildTask, "Ctrl+Shift+B"),
            Keyed("Test", Command::TestTask, "Ctrl+Shift+T"),
            Item("Test the assembly routines (Z80, 6502)", Command::AssemblyTests),
            Item("Outdated packages\u{2026}", Command::OutdatedPackages),
            Keyed("Send the HTTP request at the cursor", Command::SendRequest, "Ctrl+Alt+R"),
            Item("Clear the HTTP cookies", Command::HttpClearCookies),
            Item("Edit the project's tasks\u{2026}", Command::EditTasks),
            Item("Microcontroller flashing\u{2026}", Command::FlashSettings),
        ];

pub(super) const GIT: &[Entry] = &[
            Keyed("Source control", Command::SourceControl, "Ctrl+Shift+G"),
            Item("Compare with the last commit", Command::GitCompareHead),
            Item("History of this file\u{2026}", Command::GitHistory),
            Check("Show who changed each line", Flag::GitBlame),
            Item("Put the change at the cursor in the next commit", Command::GitStageChange),
            Item("Undo the change at the cursor", Command::GitUndoChange),
            Line,
            Item("Resolve in merge\u{2026}", Command::ResolveInMerge),
            Item("Merge three files\u{2026}", Command::Merge),
            Item("Use Limoniter for Git diff and merge", Command::GitUseTools),
            Item("Stop using Limoniter for Git", Command::GitStopTools),
            Line,
            Item("Git wizard\u{2026}", Command::GitWizard),
            Item("Copy a project from online (clone)\u{2026}", Command::GitClone),
        ];

pub(super) const ASSISTANT: &[Entry] = &[
            Check("Assistant panel", Flag::AssistantPanel),
            Line,
            Keyed("Fix\u{2026}", Command::Assist(assistant::Task::Fix), "Ctrl+Shift+A"),
            Item("Clean up\u{2026}", Command::Assist(assistant::Task::CleanUp)),
            Item("Write code\u{2026}", Command::Assist(assistant::Task::Write)),
            Item("Explain\u{2026}", Command::Assist(assistant::Task::Explain)),
            Item("Ask about it\u{2026}", Command::Assist(assistant::Task::Ask)),
        ];

pub(super) const RUN: &[Entry] = &[
            Keyed("Run a command\u{2026}", Command::RunTool, "F5"),
            Item("Stop it", Command::StopTool),
            Keyed("Debug (Python, PHP, C, C++, Rust)", Command::StartDebugging, "Ctrl+F5"),
            Item("Debug PHP from a page in the browser", Command::DebugPhpListen),
            Item("Debug with a VS Code setup\u{2026}", Command::DebugVscodeSetup),
            Keyed("Step back", Command::DebugStepBack, "Shift+F10"),
            Line,
            Item("Serve this folder in a browser (live server)", Command::ServeFolder),
            Line,
            Check("Output panel", Flag::OutputWindow),
        ];

/// Jupyter notebooks: running cells and changing them. In a notebook's own tab Shift+Enter, Ctrl+Enter and the
/// command-mode letters do the same.
pub(super) const NOTEBOOK: &[Entry] = &[
    Keyed("Run cell and go to the next", Command::Notebook(NotebookAct::RunCell), "Shift+Enter"),
    Keyed("Run cell and stay", Command::Notebook(NotebookAct::RunCellStay), "Ctrl+Enter"),
    Keyed("Run cell and add one below", Command::Notebook(NotebookAct::RunCellInsert), "Alt+Enter"),
    Line,
    Item("Run all cells", Command::Notebook(NotebookAct::RunAll)),
    Item("Run all cells above", Command::Notebook(NotebookAct::RunAbove)),
    Item("Run this cell and all below", Command::Notebook(NotebookAct::RunBelow)),
    Line,
    Item("Interrupt the kernel", Command::Notebook(NotebookAct::Interrupt)),
    Item("Restart the kernel", Command::Notebook(NotebookAct::Restart)),
    Item("Restart the kernel and run all cells", Command::Notebook(NotebookAct::RestartRunAll)),
    Line,
    Item("Clear this cell's output", Command::Notebook(NotebookAct::ClearCellOutput)),
    Item("Clear all outputs", Command::Notebook(NotebookAct::ClearOutputs)),
    Item("Hide or show this cell's output", Command::Notebook(NotebookAct::ToggleOutput)),
    Line,
    Item("Add a cell above", Command::Notebook(NotebookAct::InsertAbove)),
    Item("Add a cell below", Command::Notebook(NotebookAct::InsertBelow)),
    Item("Delete this cell", Command::Notebook(NotebookAct::Delete)),
    Item("Move this cell up", Command::Notebook(NotebookAct::MoveUp)),
    Item("Move this cell down", Command::Notebook(NotebookAct::MoveDown)),
    Keyed("Split this cell at the cursor", Command::Notebook(NotebookAct::Split), "Ctrl+Shift+-"),
    Item("Join this cell with the next", Command::Notebook(NotebookAct::Join)),
    Line,
    Item("Make this cell code", Command::Notebook(NotebookAct::ToCode)),
    Item("Make this cell Markdown", Command::Notebook(NotebookAct::ToMarkdown)),
    Item("Make this cell raw text", Command::Notebook(NotebookAct::ToRaw)),
    Line,
    Item("Compare with the last commit, cell by cell", Command::Notebook(NotebookAct::CompareLast)),
    Item("Compare with the last commit, with outputs", Command::Notebook(NotebookAct::CompareLastWithOutputs)),
];

/// Databases: SQL run from the editor on the connection chosen for the file, and the connections themselves.
pub(super) const DATABASE: &[Entry] = &[
    Keyed("Run the statement at the cursor", Command::Database(DatabaseAct::Run), "Ctrl+Enter"),
    Item("Stop the query", Command::Database(DatabaseAct::Stop)),
    Line,
    Item("Add a connection\u{2026}", Command::Database(DatabaseAct::Add)),
    Check("Database panel", Flag::Panel(PanelFlag::Database)),
];
/// Python and R: the console beside the editor, a script's `# %%` cells, environments, and knitting.
pub(super) const PYTHON_R: &[Entry] = &[
            Keyed("Send the line or selection to the console", Command::PyR(PyAction::SendToConsole), "Ctrl+Enter"),
            Keyed("Run the cell", Command::PyR(PyAction::RunCell), "Ctrl+Shift+Enter"),
            Item("Run the cells above, and this one", Command::PyR(PyAction::RunCellsAbove)),
            Item("Run all cells", Command::PyR(PyAction::RunAllCells)),
            Line,
            Check("Console", Flag::Panel(PanelFlag::Console)),
            Item("Interrupt the console", Command::PyR(PyAction::InterruptConsole)),
            Item("Restart the console", Command::PyR(PyAction::RestartConsole)),
            Item("Clear the console", Command::PyR(PyAction::ClearConsole)),
            Line,
            Item("Make a Python environment (.venv)", Command::PyR(PyAction::MakeEnvironment)),
            Item("Install the project's packages", Command::PyR(PyAction::InstallPackages)),
            Item("Look for Python environments again", Command::PyR(PyAction::FindEnvironments)),
            Line,
            Item("Knit / Render to HTML", Command::PyR(PyAction::KnitRender)),
        ];

/// The chores of keeping old PHP working, and checking and running it.
pub(super) const PHP: &[Entry] = &[
            Item("Short tags made long\u{2026}", Command::Php(PhpAction::LongTags)),
            Item("Fix \u{201C}headers already sent\u{201D}\u{2026}", Command::Php(PhpAction::HeadersSent)),
            Item("array() to []\u{2026}", Command::Php(PhpAction::ShortArrays)),
            Item("PHP 7 and 8 upgrade check\u{2026}", Command::Php(PhpAction::Upgrade)),
            Item("Old Latin-1 and Windows-1252 files made UTF-8\u{2026}", Command::Php(PhpAction::Utf8)),
            Line,
            Item("Check this file (php -l)", Command::Php(PhpAction::LintNow)),
            Check("Check each PHP file as it's saved", Flag::PhpLintOnSave),
            Line,
            Item("Run as a script", Command::Php(PhpAction::RunScript)),
            Item("Serve the folder and open it in the browser", Command::Php(PhpAction::Serve)),
            Item("Stop serving", Command::Php(PhpAction::StopServing)),
        ];

pub(super) const MACROS: &[Entry] = &[
            Keyed("Record or stop", Command::RecordMacro, "Ctrl+Shift+R"),
            Keyed("Play", Command::PlayMacro, "Ctrl+Shift+P"),
            Item("Play several times\u{2026}", Command::PlayMacroTimes),
            Entry::Live("Play a saved macro", Live::Macros),
            Line,
            Item("Load\u{2026}", Command::LoadMacro),
            Check("Macros panel", Flag::Panel(PanelFlag::Macros)),
        ];

pub(super) const SCRIPTS: &[Entry] = &[Entry::Live("Run", Live::Scripts), Line, Item("New script\u{2026}", Command::NewScript), Item("Load\u{2026}", Command::LoadScript), Check("Scripts panel", Flag::Panel(PanelFlag::Scripts))];

pub(super) const ADD_ONS: &[Entry] = &[Entry::Live("Commands", Live::AddOns), Line, Item("Manage add-ons\u{2026}", Command::ManageAddOns), Item("New add-on\u{2026}", Command::NewAddOn)];

pub(super) const PROJECT: &[Entry] = &[
            Item("Open a folder as a project\u{2026}", Command::OpenFolder),
            Item("Open a remote folder as a project\u{2026}", Command::OpenRemoteFolder),
            Item("Open a ZIP as a project\u{2026}", Command::OpenZipProject),
            Item("Open a project from a web address (a ZIP or Git)\u{2026}", Command::OpenWebProject),
            Item("New project\u{2026}", Command::ProjectNew),
            Item("New Amiga C project\u{2026}", Command::AmigaNewProject),
            Item("Import an Amiga C project\u{2026}", Command::AmigaImportProject),
            Item("Open project\u{2026}", Command::ProjectOpen),
            Entry::Live("Open recent", Live::RecentProjects),
            Entry::Live("Save files by themselves", Live::ProjectAutosave),
            Item("Close project", Command::ProjectClose),
            Item("Project settings\u{2026}", Command::ProjectSettings),
            Line,
            Item("Add this file", Command::ProjectAddActive),
            Item("New from a template\u{2026}", Command::NewFromTemplate(0)),
            Item("Save this file as a template\u{2026}", Command::SaveAsTemplate),
            Item("Add files\u{2026}", Command::ProjectAddFiles),
            Item("Add a folder\u{2026}", Command::ProjectAddFolder),
            Line,
            Item("Fetch the server's changes", Command::RemoteProjectFetch),
            Item("Sync with the server\u{2026}", Command::RemoteProjectSync),
            Item("Open a terminal here", Command::RemoteProjectTerminal),
            Line,
            Item("Back up", Command::ProjectBackup),
            Item("Restore from a backup\u{2026}", Command::ProjectRestore),
            Check("Project panel", Flag::Panel(PanelFlag::Project)),
            Check("Files changing", Flag::Panel(PanelFlag::FilesChanging)),
        ];

pub(super) const RUN_ON: &[Entry] = &[
            Entry::Live("Machines", Live::Machines),
            Line,
            Item("Its memory", Command::MachineMemory),
            Item("Trainer\u{2026}", Command::Trainer),
            Item("Its screen as text", Command::MachineScreen),
            Item("Take its BASIC program", Command::MachineBasicOut),
            Item("Put this listing into it", Command::MachineBasicIn),
            Line,
            Item("Stop running", Command::StopRunning),
            Line,
            Item("Assemble and save", Command::AssembleAndSave),
            Item("Assemble and save as binary\u{2026}", Command::AssembleBinary),
            Item("Assemblers\u{2026}", Command::ChooseAssembler),
            Item("Compilers\u{2026}", Command::Compilers),
            Item("Amiga NDK (code hints)\u{2026}", Command::AmigaNdk),
            Item("Amiga crunchers and music\u{2026}", Command::AmigaTools),
            Item("Profile in Hatari", Command::HatariProfile),
            Item("Make a bootable ST floppy\u{2026}", Command::StFloppy),
            Item("Devices and emulators\u{2026}", Command::ManageDevices),
            Item("Set up the Amiga\u{2026}", Command::AmigaSetup),
        ];

pub(super) const SEND_TO: &[Entry] = &[Entry::Live("Devices", Live::Devices), Line, Keyed("Send again", Command::SendAgain, "Ctrl+Alt+S"), Item("Manage devices\u{2026}", Command::ManageDevices)];

pub(super) const THIS_FILE: &[Entry] = &[
            Item("Rename\u{2026}", Command::RenameFile),
            Item(crate::trash::MOVE_TO_BIN, Command::MoveToBin),
            Item(crate::platform::SHOW_IN_FILE_MANAGER, Command::ShowInExplorer),
            Item("Open in its usual program", Command::OpenWithDefault),
            Line,
            Item("Split into parts\u{2026}", Command::SplitFile),
        ];

pub(super) const LAYOUTS: &[Entry] = &[Entry::Live("Use", Live::Layouts), Line, Item("Save this layout\u{2026}", Command::SaveLayout), Sub("Delete", &[Entry::Live("Delete a layout", Live::DeleteLayouts)])];

/// File, Export: the open file made into something else, a picture, a tape's sound, or a tokenised program.
pub(super) const EXPORT: &[Entry] = &[
            Item("Code as a picture\u{2026}", Command::CodePicture),
            Item("Make its tape sound (WAV)", Command::TapeSound),
            Line,
            Sub("Tokenised BASIC", TOKENISED_BASIC),
        ];

pub(super) const FILE: &[Entry] = &[
    Keyed("New", Command::New, "Ctrl+N"),
    Entry::Live("New as", Live::NewAs),
    Item("New disk or tape image\u{2026}", Command::NewImage),
    Keyed("Open\u{2026}", Command::Open, "Ctrl+O"),
    Item("Open folder\u{2026}", Command::OpenFolder),
    Keyed("Go to file\u{2026}", Command::GoToFile, "Ctrl+E"),
    Entry::Live("Open recent", Live::RecentFiles),
    Item("Open a file from the web (http/https)\u{2026}", Command::OpenUrl),
    Item("Open a remote file (SFTP or FTP)\u{2026}", Command::OpenRemote),
    Item("Open a remote folder as a project (SFTP or FTP)\u{2026}", Command::OpenRemoteFolder),
    Item("Open a terminal on a server (SSH)\u{2026}", Command::OpenSshTerminal),
    Item("Open a ZIP as a project\u{2026}", Command::OpenZipProject),
    Item("Open a project from a web address (a ZIP or Git)\u{2026}", Command::OpenWebProject),
    Line,
    Keyed("Save", Command::Save, "Ctrl+S"),
    Keyed("Save as\u{2026}", Command::SaveAs, "Ctrl+Shift+S"),
    Item("Save the selection as\u{2026}", Command::SaveSelection),
    Item("Save a copy in another encoding\u{2026}", Command::SaveCopyAs),
    Item("Save to a server (SFTP or FTP)\u{2026}", Command::SaveRemote),
    Sub("Export", EXPORT),
    Sub("Send to device", SEND_TO),
    Keyed("Run on machine", Command::RunOnMachine, "F9"),
    Sub("Run on", RUN_ON),
    Line,
    Sub("This file", THIS_FILE),
    Item("Reopen with another encoding\u{2026}", Command::ReopenAs),
    Item("Recover unsaved edits\u{2026}", Command::RecoverEdits),
    Line,
    Keyed("Close", Command::CloseTab, "Ctrl+W"),
    Item("Close all", Command::CloseAll),
    Item("Close the others", Command::CloseOthers),
    Line,
    Keyed("Print\u{2026}", Command::Print, "Ctrl+P"),
    Line,
    Item("New window", Command::NewInstance),
    Keyed("Quit", Command::Quit, "Ctrl+Q"),
];

pub(super) const EDIT: &[Entry] = &[
    Keyed("Undo", Command::Undo, "Ctrl+Z"),
    Keyed("Redo", Command::Redo, "Ctrl+Y"),
    Keyed("Edit history", Command::ShowHistory, "Ctrl+Shift+H"),
    Keyed("History of this part", Command::PartHistory, "Ctrl+Alt+H"),
    Line,
    Keyed("Cut", Command::Cut, "Ctrl+X"),
    Keyed("Copy", Command::Copy, "Ctrl+C"),
    Keyed("Paste", Command::Paste, "Ctrl+V"),
    Keyed("Swap with clipboard", Command::SwapClipboard, "Ctrl+Shift+V"),
    Sub("Clipboards", CLIPBOARDS),
    Line,
    Keyed("Select all", Command::SelectAll, "Ctrl+A"),
    Sub("Select", SELECT),
    Sub("Replace inside", &REPLACE_INSIDE),
    Sub("Delete", &[Sub("Inside", &DELETE_INSIDE), Sub("Around", &DELETE_AROUND)]),
    Line,
    Sub("Lines", LINES),
    Sub("Columns", COLUMNS),
    Sub("Change case", CHANGE_CASE),
    Sub("Insert", INSERT),
    Sub("Snippets", &[Keyed("Choose or make one\u{2026}", Command::Snippets, "Ctrl+J"), Line, Entry::Live("Insert", Live::Snippets)]),
    Sub("Surround with", SURROUND),
    Line,
    Check("Read-only", Flag::ReadOnly),
    Entry::Live("Claude Code", Live::ClaudeCode),
];

pub(super) const SEARCH: &[Entry] = &[
    Keyed("Find\u{2026}", Command::Find, "Ctrl+F"),
    Keyed("Replace\u{2026}", Command::Replace, "Ctrl+R"),
    Keyed("Find next", Command::FindNext, "F3"),
    Keyed("Find previous", Command::FindPrevious, "Shift+F3"),
    Keyed("Search panel (in the folder's files)", Command::SearchPanel, "Ctrl+Shift+F"),
    Item("Find in files\u{2026}", Command::FindInFiles),
    Line,
    Item("Only show lines with the selection", Command::FilterSelection),
    Line,
    Keyed("Go to line\u{2026}", Command::GoTo, "Ctrl+G"),
    Keyed("Go to symbol in this file\u{2026}", Command::GoToSymbol(false), "Ctrl+Shift+O"),
    Keyed("Go to symbol in the folder\u{2026}", Command::GoToSymbol(true), "Ctrl+T"),
    Keyed("Go to definition", Command::GoToDefinition, "F12"),
    Keyed("Peek definition", Command::PeekDefinition, "Alt+F12"),
    Keyed("Find all uses", Command::FindUses, "Shift+F12"),
    Keyed("Quick fix\u{2026}", Command::QuickFix, "Ctrl+."),
    Keyed("Rename\u{2026}", Command::RenameLabel, "Ctrl+F12"),
    Item("Go to the matching bracket", Command::GoToBrace),
    Keyed("Back", Command::NavBack, "Alt+Left"),
    Keyed("Forward", Command::NavForward, "Alt+Right"),
    Item("Previous change", Command::LastChange),
    Item("Next change", Command::NextChange),
    Line,
    Sub("Bookmarks", BOOKMARKS),
    Sub("Colour tags", &[Keyed("Next", Command::NextTag, "F4"), Keyed("Previous", Command::PreviousTag, "Shift+F4"), Line, Entry::Live("Go to", Live::Tags)]),
];

pub(super) const VIEW: &[Entry] = &[
    Item("Larger text", Command::ZoomIn),
    Item("Smaller text", Command::ZoomOut),
    Item("Font\u{2026}", Command::ChooseFont),
    Item("Window font and size\u{2026}", Command::WindowFont),
    Sub("Theme", THEME),
    Line,
    Check("Word wrap", Flag::WordWrap),
    Sub("Show", SHOW),
    Sub("Folding", FOLDING),
    Keyed("Narrow to the selection", Command::Narrow, "Ctrl+Shift+N"),
    Item("Show the whole file", Command::Widen),
    Keyed("Table view (CSV, TSV)", Command::ToggleTable, "F6"),
    Item("Open in the data viewer", Command::OpenInDataViewer),
    Item("Notebook as text", Command::Notebook(NotebookAct::AsText)),
    Line,
    Sub("Preview", PREVIEW),
    Sub("Hex", HEX),
    Sub("Split", SPLIT),
    Line,
    Sub("Panels", PANELS),
    Sub("Colour tag", COLOUR_TAG),
    Check("Output", Flag::OutputWindow),
    Keyed("Terminal", Command::ToggleTerminal, "Ctrl+`"),
    Item("Limoniter command in WSL\u{2026}", Command::WslCommands),
    Check("File tabs", Flag::FileTabs),
    Check("Tabs down the side", Flag::SideTabs),
    Check("Side tabs on the right", Flag::SideTabsRight),
    Check("Toolbar", Flag::Toolbar),
    Item("Toolbar buttons\u{2026}", Command::ToolbarButtons),
    Check("Status bar", Flag::StatusBar),
    Check("Ribbon", Flag::Ribbon),
    Keyed("Fold the ribbon to its tabs", Command::FoldRibbon, "Ctrl+F1"),
    Line,
    Check("Always on top", Flag::AlwaysOnTop),
    Check("Full screen", Flag::FullScreen),
];

pub(super) const FORMAT: &[Entry] = &[
    Sub("Encoding", ENCODING),
    Item("Open the BASIC listing as text", Command::OpenBasicListing),
    Item("Save the picture as PNG\u{2026}", Command::SavePictureAsPng),
    Item("Save the preview as PDF\u{2026}", Command::SavePreviewAsPdf),
    Sub("Save the document as", SAVE_DOCUMENT_AS),
    Line,
    Item("Reindent", Command::Reindent),
    Sub("Tabs and spaces", TABS_AND_SPACES),
    Line,
    Sub("Paragraph", PARAGRAPH),
    Keyed("Format document", Command::FormatDocument, "Shift+Alt+F"),
    Item("Formatters\u{2026}", Command::Formatters),
    Sub("Lay out code", LAY_OUT_CODE),
    Sub("Compress", COMPRESS),
    Sub("Convert", CONVERT),
    Sub("Table", TABLE),
    Line,
    Item("Comment out", Command::Comment),
    Item("Uncomment", Command::Uncomment),
    Item("Doc comment for this function", Command::DocComment),
    Line,
    Item("Markdown: table of contents", Command::MarkdownToc),
    Item("Markdown: line up the table", Command::MarkdownTable),
];

/// Code: the tools for modern coding, as the ribbon's Code tab has them.
pub(super) const CODE: &[Entry] = &[
    Sub("Tasks", TASKS),
    Sub("Run", RUN),
    Sub("Git", GIT),
    Sub("Python and R", PYTHON_R),
    Sub("Notebook", NOTEBOOK),
    Sub("Database", DATABASE),
    Sub("PHP", PHP),
    Sub("Assistant", ASSISTANT),
    Sub("Project", PROJECT),
    Line,
    Item("Query the rows\u{2026}", Command::QueryRows),
    Item("Regular expression tester", Command::RegexTester),
    Item("Check JSON or YAML against a schema", Command::SchemaCheck),
];

/// Retro: the tools for the older machines, one sub-menu a machine, as the ribbon's Retro tab has them.
pub(super) const RETRO: &[Entry] = &[
    Sub("General", GENERAL),
    Sub("Hardware", HARDWARE),
    Sub("Amiga", AMIGA),
    Sub("Atari 2600", ATARI_2600),
    Sub("PICO-8 and TIC-80", FANTASY),
    Sub("Atari ST", ATARI_ST),
    Sub("Commodore 64", C64),
    Sub("Dragon and Vectrex", DRAGON),
    Sub("ZX Spectrum and Next", SPECTRUM),
    Sub("Early computing", EARLY),
];

/// Rhymes, a thesaurus, metre and verse forms, and marks for prose (see `words_host`).
pub(super) const WORDS: &[Entry] = &[
    Keyed("Rhymes for the word at the cursor", Command::Words(WordsAction::Rhymes), "Ctrl+F7"),
    Keyed("Thesaurus for the word at the cursor", Command::Words(WordsAction::Thesaurus), "Shift+F7"),
    Line,
    Item("Stress and metre of the selected lines", Command::Words(WordsAction::Metre)),
    Item("Check as a haiku", Command::Words(WordsAction::Haiku)),
    Item("Check as a limerick", Command::Words(WordsAction::Limerick)),
    Line,
    Check("Syllable counts beside the lines of this file", Flag::Words(WordsSwitch::SyllablesHere)),
    Check("Syllable counts in every text and Markdown file", Flag::Words(WordsSwitch::SyllablesText)),
    Check("Mark words that come round again too soon", Flag::Words(WordsSwitch::Repeats)),
    Check("Mark filler words", Flag::Words(WordsSwitch::Filler)),
    Check("Mark adverbs", Flag::Words(WordsSwitch::Adverbs)),
];

/// What a song's file gives Suno and other song makers (see `lyrics_host`).
pub(super) const LYRICS: &[Entry] = &[
    Item("Copy the lyrics for Suno (with their tags)", Command::Lyrics(crate::app::lyrics_host::Action::CopyLyrics)),
    Item("Copy the style for Suno", Command::Lyrics(crate::app::lyrics_host::Action::CopyStyle)),
    Item("Copy the words alone (no tags)", Command::Lyrics(crate::app::lyrics_host::Action::CopyPlain)),
];

pub(super) const TOOLS: &[Entry] = &[
    Item("Colour as\u{2026}", Command::ColourAs),
    Keyed("Colour picker\u{2026}", Command::ColourPicker, "Ctrl+Alt+K"),
    Line,
    Item("Compare with\u{2026}", Command::Compare),
    Item("Compare files\u{2026}", Command::CompareFiles),
    Item("Compare folders\u{2026}", Command::CompareFolders),
    Item("Saved comparisons\u{2026}", Command::CompareSessions),
    Item("Apply a patch\u{2026}", Command::ApplyPatch),
    Item("Count words", Command::WordCount),
    Item("Fingerprint (SHA-256)", Command::ComputeHash),
    Item("Time spent\u{2026}", Command::TimeSpent),
    Item("Convert a number", Command::ConvertNumber),
    Item("New AI agent session\u{2026}", Command::AgentNew),
    Item("Usage of AI tools\u{2026}", Command::Usage),
    Item("Copy the file's path", Command::CopyPath),
    Line,
    Sub("Spelling", &[Keyed("Next mistake", Command::NextMistake, "F7"), Check("Underline mistakes", Flag::Spelling)]),
    Sub("Words", WORDS),
    Sub("Song lyrics", LYRICS),
    Sub("Macros", MACROS),
    Sub("Scripts", SCRIPTS),
    Sub("Add-ons", ADD_ONS),
    Item("VS Code extensions\u{2026}", Command::Extensions),
    Line,
    Keyed("Settings\u{2026}", Command::Settings, "Ctrl+,"),
    Item("Save settings to a file\u{2026}", Command::SaveSettingsFile),
    Item("Restore settings from a file\u{2026}", Command::RestoreSettingsFile),
];

pub(super) const WINDOW: &[Entry] = &[
    Keyed("Last file used", Command::UsedFileBack, "Ctrl+Tab"),
    Keyed("Least recently used file", Command::UsedFileForward, "Ctrl+Shift+Tab"),
    Item("Previous tab", Command::PreviousTab),
    Item("Next tab", Command::NextTab),
    Check("List of open files", Flag::Panel(PanelFlag::OpenFiles)),
    Item("Recently opened files", Command::ShowFileLists),
    Line,
    Sub("Layouts", LAYOUTS),
    Item("Gather windows", Command::GatherWindows),
    Item("New window", Command::NewInstance),
];

pub(super) const HELP_MENU: &[Entry] = &[Keyed("Find a command\u{2026}", Command::CommandPalette, "F1"), Item("How things work", Command::Help), Sub("Guide", GUIDE), Item("What's new in this version", Command::WhatsNew), Item("What Limoniter can do", Command::Features), Item("How To Code: writing Amiga demos", Command::HowToCode), Item("How To Code: the ZX Spectrum", Command::HowToCodeSpectrum), Item("Limoniter website", Command::Website), Item("Check for a newer Limoniter", Command::CheckUpdates), Item("Go back to the previous version", Command::GoBackVersion), Item("Check for a newer OpticOS ROM", Command::CheckOpticos), Line, Item("About Limoniter", Command::About)];

/// The guide's pages, each a short walk through one kind of work.
pub(super) const GUIDE: &[Entry] = &[Item("Getting started", Command::Guide(Guide::GettingStarted)), Item("Working on a folder of code", Command::Guide(Guide::Folder)), Item("Assembling and running", Command::Guide(Guide::Assembling))];

pub(super) const MENUS: [(&str, &[Entry]); 10] = [("File", FILE), ("Edit", EDIT), ("Search", SEARCH), ("View", VIEW), ("Format", FORMAT), ("Code", CODE), ("Retro", RETRO), ("Tools", TOOLS), ("Window", WINDOW), ("Help", HELP_MENU)];

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
    /// A recent file, and the project it was in, to be opened first.
    RecentFile(PathBuf, Option<PathBuf>),
    Project(PathBuf),
    Play(PathBuf),
    AddOn(usize),
    Device(usize),
    RunOn(Option<usize>),
    WindowTheme(String),
    DocumentTheme(String),
    ThisDocumentTheme(Option<String>),
    Snippet(usize),
    Line(usize),
    UseLayout(usize),
    DeleteLayout(usize),
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

/// Commands found only by typing their whole name, never listed.
pub(super) fn hidden_commands(query: &str) -> Vec<Found> {
    if query.trim().eq_ignore_ascii_case("clambers") {
        vec![Found { path: "Clambers".into(), shortcut: "", command: Command::Clambers }]
    } else {
        Vec::new()
    }
}

/// Every command, with where it is in the menus, so the command box can find it by any of those words.
pub(super) fn every_command() -> Vec<Found> {
    commands_listed(true)
}

/// The same list with every name in English, whatever language is chosen: what's kept in the settings to say which
/// command a toolbar button is, so changing language doesn't lose the buttons.
pub(super) fn every_command_in_english() -> Vec<Found> {
    commands_listed(false)
}

fn commands_listed(translated: bool) -> Vec<Found> {
    fn walk(entries: &[Entry], path: &str, translated: bool, out: &mut Vec<Found>) {
        for entry in entries {
            let named = |label: &str| {
                let label = if translated { crate::i18n::tr(label).to_string() } else { label.to_string() };
                if path.is_empty() { label } else { format!("{path} \u{203A} {label}") }
            };
            match entry {
                Item(label, command) => out.push(Found { path: named(label), shortcut: "", command: *command }),
                Keyed(label, command, key) => out.push(Found { path: named(label), shortcut: key, command: *command }),
                Check(label, flag) => out.push(Found { path: named(label), shortcut: "", command: Command::Toggle(*flag) }),
                Sub(label, inner) => walk(inner, &named(label), translated, out),
                Entry::Live(..) | Line => {}
            }
        }
    }
    let mut out = Vec::new();
    for (title, entries) in MENUS {
        walk(entries, &if translated { crate::i18n::tr(title).to_string() } else { title.to_string() }, translated, &mut out);
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

/// Commands that read or rewrite the file's text as text, so make no sense while it's shown as something else.
fn works_on_text(command: Command) -> bool {
    matches!(command, Command::Format(_) | Command::FormatDocument | Command::GraphMode | Command::UdgPictures | Command::OpenBasicListing)
        || TOKENISED_BASIC.iter().any(|entry| matches!(entry, Item(_, c) | Keyed(_, c, _) if *c == command))
}

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
            PanelFlag::Todos => p.todos,
            PanelFlag::FilesChanging => p.files_changing,
            PanelFlag::Agents => p.agents,
            PanelFlag::FileProperties => p.file_properties,
            PanelFlag::Json => p.json_manager,
            PanelFlag::Xml => p.xml_manager,
            PanelFlag::Characters => p.ascii_table,
            PanelFlag::EncodingPreview => p.encoding_preview,
            PanelFlag::Clipboard => p.clipboard_history,
            PanelFlag::Strings => p.strings,
            PanelFlag::Inspector => p.inspector,
            PanelFlag::Structure => p.structure,
            PanelFlag::CallGraph => p.call_graph,
            PanelFlag::Instructions => p.instructions,
            PanelFlag::Variables => p.variables,
            PanelFlag::Database => p.database,
            PanelFlag::Console => self.python.open,
            PanelFlag::Tests => p.tests,
            PanelFlag::Containers => p.containers,
        }
    }

    fn panel_switch(&mut self, panel: PanelFlag) -> &mut bool {
        if panel == PanelFlag::Console {
            return &mut self.python.open;
        }
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
            PanelFlag::Todos => &mut p.todos,
            PanelFlag::FilesChanging => &mut p.files_changing,
            PanelFlag::Agents => &mut p.agents,
            PanelFlag::FileProperties => &mut p.file_properties,
            PanelFlag::Json => &mut p.json_manager,
            PanelFlag::Xml => &mut p.xml_manager,
            PanelFlag::Characters => &mut p.ascii_table,
            PanelFlag::EncodingPreview => &mut p.encoding_preview,
            PanelFlag::Clipboard => &mut p.clipboard_history,
            PanelFlag::Strings => &mut p.strings,
            PanelFlag::Inspector => &mut p.inspector,
            PanelFlag::Structure => &mut p.structure,
            PanelFlag::CallGraph => &mut p.call_graph,
            PanelFlag::Instructions => &mut p.instructions,
            PanelFlag::Variables => &mut p.variables,
            PanelFlag::Database => &mut p.database,
            PanelFlag::Console => unreachable!("kept by python_host, above"),
            PanelFlag::Tests => &mut p.tests,
            PanelFlag::Containers => &mut p.containers,
        }
    }

    pub(super) fn flag_on(&self, flag: Flag) -> bool {
        let doc = &self.docs[self.active];
        let v = &self.view;
        match flag {
            Flag::LineNumbers => v.line_numbers,
            Flag::Ruler => v.ruler,
            Flag::ColumnMarkers => v.column_markers,
            Flag::SpacesForTab => self.prefs.edit.spaces_for_tab,
            Flag::DocumentMap => v.document_map,
            Flag::Breadcrumbs => v.breadcrumbs,
            Flag::IndentGuides => v.indent_guides,
            Flag::HideComments => v.hide_comments,
            Flag::PanelsHide => v.panels_hide,
            Flag::StackPanels => self.panels.stacked,
            Flag::StickyScroll => v.sticky_scroll,
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
            Flag::SideTabs => self.side_tabs.on,
            Flag::SideTabsRight => self.side_tabs.right,
            Flag::Toolbar => self.toolbar.on,
            Flag::StatusBar => self.show_status,
            Flag::OutputWindow => self.show_results,
            Flag::WordWrap => v.word_wrap,
            Flag::ReadOnly => doc.text.read_only,
            Flag::ColumnMode => !doc.hex_mode && doc.text.column_mode(),
            Flag::AlwaysOnTop => self.always_on_top,
            Flag::FullScreen => self.full_screen,
            Flag::Spelling => self.prefs.spelling,
            Flag::Words(switch) => crate::app::words_host::switch_on(switch, &doc.text),
            Flag::GitBlame => self.git.blame_on,
            Flag::CycleCounts => crate::m6502_timing::shown(),
            Flag::AssistantPanel => self.assist_panel.open,
            Flag::Ribbon => v.ribbon,
            Flag::PhpLintOnSave => self.php.lint_on_save,
            Flag::Panel(panel) => self.panel_on(panel),
        }
    }

    pub(super) fn toggle_flag(&mut self, flag: Flag, ui: &Ui) {
        let v = &mut self.view;
        let switch: &mut bool = match flag {
            Flag::LineNumbers => &mut v.line_numbers,
            Flag::Ruler => &mut v.ruler,
            Flag::ColumnMarkers => &mut v.column_markers,
            Flag::SpacesForTab => &mut self.prefs.edit.spaces_for_tab,
            Flag::DocumentMap => &mut v.document_map,
            Flag::Breadcrumbs => &mut v.breadcrumbs,
            Flag::IndentGuides => &mut v.indent_guides,
            Flag::HideComments => &mut v.hide_comments,
            Flag::PanelsHide => &mut v.panels_hide,
            Flag::StackPanels => &mut self.panels.stacked,
            Flag::StickyScroll => &mut v.sticky_scroll,
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
            Flag::SideTabs => &mut self.side_tabs.on,
            Flag::SideTabsRight => &mut self.side_tabs.right,
            Flag::Toolbar => &mut self.toolbar.on,
            Flag::StatusBar => &mut self.show_status,
            Flag::OutputWindow => &mut self.show_results,
            Flag::Ribbon => &mut v.ribbon,
            Flag::GitBlame => {
                self.git.blame_on = !self.git.blame_on;
                return;
            }
            Flag::Words(switch) => {
                crate::app::words_host::toggle(switch, &mut self.docs[self.active].text);
                return;
            }
            Flag::PhpLintOnSave => {
                self.php.lint_on_save = !self.php.lint_on_save;
                return;
            }
            Flag::CycleCounts => {
                crate::m6502_timing::show(!crate::m6502_timing::shown());
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
        // Tabs down the side are tabs, so choosing them shows the tabs.
        if flag == Flag::SideTabs && self.side_tabs.on {
            self.show_file_tabs = true;
        }
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
        // A file shown as something other than its text (a notebook, a table, the data viewer, a picture or a disk)
        // isn't offered what works on the text underneath: tokenising it as BASIC, or reformatting it.
        let shown_otherwise = (doc.notebook.is_some() && !doc.notebook_text) || doc.table.is_some() || doc.data.is_some() || doc.browser.is_some();
        if shown_otherwise && works_on_text(command) {
            return false;
        }
        match command {
            Command::Undo => if hex { doc.hex.can_undo() } else { t.can_undo() },
            Command::Redo => if hex { doc.hex.can_redo() } else { t.can_redo() },
            Command::Copy | Command::Cut => selected,
            Command::GoBackVersion => self.can_go_back(),
            Command::FindNext | Command::FindPrevious => !self.find.find.is_empty(),
            Command::NavBack => self.jumps.can_go_back(),
            Command::NavForward => self.jumps.can_go_forward(),
            Command::LastChange | Command::NextChange => text && t.changed_line_count() > 0,
            Command::NextBookmark | Command::PreviousBookmark => text && !t.bookmarks().is_empty(),
            Command::CopyPath | Command::RenameFile | Command::ShowInExplorer | Command::OpenWithDefault => doc.path.is_some(),
            Command::SendTo(n) if matches!(self.sends.devices.get(n).map(|d| &d.place), Some(crate::deploy::Place::Emulator { .. })) => !no_file,
            Command::SendTo(_) | Command::SendAgain => !no_file && doc.browser.is_none(),
            Command::SendRequest | Command::HttpClearCookies => !no_file && super::http_host::is_http_doc(doc),
            Command::RunOnMachine | Command::RunOnBuiltIn | Command::RunOnBuiltIn2600 | Command::RunOnBuiltInVectrex | Command::RunOn(_) | Command::RunInWindow(_) => !no_file,
            Command::FindUses | Command::RenameLabel => !no_file && (self.doc_cpu(self.active).is_some() || self.has_lsp()),
            Command::GoToDefinition | Command::PeekDefinition => !no_file,
            Command::QuickFix => self.has_lsp(),
            Command::FormatDocument => self.has_lsp() || (text && crate::ext_formatters::chosen_near(&doc.language_name(), &self.formatter_choices_now(), doc.path.as_deref()).is_some()),
            Command::AssembleAndSave | Command::AssembleBinary => !no_file && self.doc_cpu(self.active).is_some(),
            Command::StopRunning => !self.running.sessions.is_empty() || !self.vamos_runs.is_empty(),
            Command::DebugStepBack => self.can_step_back(),
            Command::DebugVscodeSetup => self.has_vscode_launches(),
            Command::MachineMemory | Command::MachineScreen | Command::MachineBasicOut => self.reachable().is_some(),
            Command::MachineBasicIn => self.reachable().is_some() && doc.listing_of.is_some(),
            Command::GitCompareHead | Command::GitHistory | Command::GitStageChange => self.git.repo.is_some() && doc.path.is_some(),
            Command::GitUndoChange => self.git.repo.is_some() && doc.path.is_some() && !doc.text.read_only,
            Command::ResolveInMerge => text && doc.merge.is_none() && !t.is_disk_backed(),
            Command::ToggleTable => doc.table.is_some() || (text && doc.browser.is_none() && doc.data.is_none()),
            Command::QueryRows => !no_file && !hex && doc.data.is_none() && doc.browser.is_none() && (doc.table.is_some() || doc.text.delimited.is_some() || super::table_view::is_table_name(&doc.path_or_name())),
            Command::OpenInDataViewer => !no_file && !hex && doc.data.is_none() && doc.browser.is_none() && (doc.table.is_some() || super::table_view::is_table_name(&doc.path_or_name())),
            Command::Notebook(super::notebook_host::Act::AsText) => !no_file && super::notebook_host::is_notebook_name(&doc.path_or_name()) && !hex && doc.table.is_none() && doc.browser.is_none(),
            Command::Notebook(_) => self.notebook_shown(self.active),
            Command::Database(act) => self.database_can(act),
            Command::SwapClipboard | Command::SaveSelection => if hex { doc.hex.has_selection() } else { t.has_selection() },
            Command::SavePictureAsPng => doc.picture.is_some(),
            Command::TapeSound => !no_file && ((doc.listing_of.is_some() && !hex) || crate::media::tape_sound::may_make(&doc.path_or_name().to_string_lossy())),
            Command::AmigaIcon => doc.picture == Some(crate::pictures::Format::AmigaIcon),
            Command::SavePreviewAsPdf => !no_file && Self::pdf_preview_kind(doc).is_some(),
            Command::SaveDocumentAs(_) => doc.legacy.is_some(),
            Command::GraphMode | Command::UdgPictures => !no_file && self.is_spectrum_basic(self.active),
            Command::OpenBasicListing => !t.is_disk_backed() && preview::kind_for_file(doc.path.as_deref(), "", &t.format) == Some(preview::Kind::Basic),
            Command::SaveSpectrumTape | Command::SaveNextProgram | Command::SaveCommodoreProgram | Command::SaveBbcProgram | Command::SaveBbcZ80Program | Command::SaveAmosProgram | Command::SaveStosProgram => text && !t.is_disk_backed(),
            Command::UsedFileBack | Command::UsedFileForward => self.docs.iter().filter(|doc| !doc.placeholder).count() > 1,
            Command::StopTool => self.tool_running(),
            Command::SplitFile => text && doc.path.is_some(),
            Command::MoveToBin => !no_file && doc.path.is_some() && doc.merge.is_none(),
            Command::PartHistory => text && !t.is_disk_backed() && doc.narrow.is_none(),
            Command::Untag => text && self.tag_here_to_remove(),
            Command::NextTag | Command::PreviousTag => text && !t.tags().is_empty(),
            Command::ClearBookmarks | Command::CopyBookmarked | Command::CutBookmarked | Command::DeleteBookmarked | Command::DeleteUnbookmarked => text && !t.bookmarks().is_empty(),
            Command::InvertBookmarks => text && !t.is_disk_backed(),
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
            Command::DocComment => text && crate::doc_comments::style_for(&doc.language_name()).is_some(),
            Command::MarkdownToc | Command::MarkdownTable => text && t.markdown,
            Command::Upper | Command::Lower | Command::SentenceCase | Command::TitleCase => text && t.has_selection(),
            Command::PreviousTab | Command::NextTab => self.all_tabs().len() > 1,
            Command::ProjectClose | Command::ProjectSettings | Command::ProjectAddFiles | Command::ProjectAddFolder | Command::ProjectBackup | Command::ProjectRestore => has_project,
            Command::ProjectAddActive => has_project && doc.path.is_some(),
            Command::NewFromTemplate(_) | Command::ProjectAutosave(_) => has_project,
            Command::SaveAsTemplate => has_project && !no_file && text && !doc.placeholder,
            Command::RemoteProjectFetch | Command::RemoteProjectSync => self.remote_project.is_some(),
            Command::RemoteProjectTerminal => self.remote_project.as_ref().is_some_and(|p| !p.origin.site.scheme.is_ftp()),
            Command::PlayMacro | Command::PlayMacroTimes => self.current_macro.is_some(),
            Command::ZoomIn => self.view.font_size < 40.0,
            Command::ZoomOut => self.view.font_size > 8.0,
            Command::FoldRibbon => self.view.ribbon,
            Command::Toggle(Flag::ColumnMode) => text,
            Command::Toggle(Flag::ReadOnly) => !no_file,
            // Hex mode has its own versions of these, or they're about the file rather than its text.
            Command::New
            | Command::Open
            | Command::Save
            | Command::SaveAs
            | Command::CloseTab
            | Command::CloseAll
            | Command::CloseOthers
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
            | Command::ColourPicker
            | Command::SaveSettingsFile
            | Command::SaveLayout
            | Command::GatherWindows
            | Command::CheckUpdates
            | Command::RestoreSettingsFile
            | Command::Print
            | Command::ReopenAs
            | Command::SaveCopyAs
            | Command::ChoosePreset
            | Command::InsertByte
            | Command::RecoverEdits
            | Command::ChooseFont
            | Command::WindowFont
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
            | Command::Extensions
            | Command::NewAddOn
            | Command::OpenRemote
            | Command::OpenRemoteFolder
            | Command::OpenSshTerminal
            | Command::OpenZipProject
            | Command::OpenWebProject
            | Command::ManageDevices
            | Command::AmigaSetup
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
            // A folder can be opened, searched and worked in with nothing open yet.
            | Command::OpenFolder
            | Command::GoToFile
            | Command::GoToSymbol(true)
            | Command::SearchPanel
            | Command::DragonDisk
            | Command::Checksums
            | Command::RomPatches
            | Command::Entropy
            | Command::RegexTester
            | Command::Trainer
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
            | Command::AutoDocs
            | Command::AmigaBootDisk
            | Command::AmigaNewProject
            | Command::AmigaImportProject
            | Command::AmigaMakefiles
            | Command::ToggleTerminal
            | Command::NewImage
            | Command::OpenUrl
            | Command::UseClipboard(_)
            | Command::Features
            | Command::WslCommands
            | Command::WhatsNew
            | Command::AgentNew
            | Command::Usage
            | Command::Guide(_)
            | Command::GitClone
            | Command::ToolbarButtons
            // With nothing open, it asks for a file to disassemble.
            | Command::Disassemble
            | Command::SaveDecrunched
            | Command::RasterDesigner
            | Command::Toggle(_) => true,
            Command::Php(action) => self.php_can(action),
            Command::PyR(action) => self.python_can(action),
            // Tasks run in the project's folder, so need a project or a file in one.
            Command::BuildTask | Command::TestTask | Command::OutdatedPackages | Command::DebugTask(_) => has_project || !no_file,
            Command::EditTasks => has_project,
            // Catalogs are worked on from their description or a translation.
            Command::CatalogCheck | Command::CatalogMake | Command::CatalogFlexCat => doc.path.as_deref().is_some_and(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cd") || e.eq_ignore_ascii_case("ct"))),
            Command::CatalogTranslation => doc.path.as_deref().is_some_and(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cd"))),
            _ => text,
        }
    }

    fn menu_item(&self, ui: &mut Ui, label: &str, shortcut: &str, command: Command, chosen: &mut Option<Command>) {
        #[cfg(test)]
        super::ribbon_bar::note_drawn(ui.ctx(), false, command, self.can(command), None);
        let button = Button::new(crate::i18n::tr(label)).shortcut_text(RichText::new(shown_shortcut(shortcut)).color(theme::text_faint())).min_size(egui::vec2(MENU_WIDTH - 16.0, 0.0));
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
                    #[cfg(test)]
                    super::ribbon_bar::note_drawn(ui.ctx(), false, Command::Toggle(*flag), self.can(Command::Toggle(*flag)), Some(on));
                    if ui.add_enabled(self.can(Command::Toggle(*flag)), egui::Checkbox::new(&mut on, crate::i18n::tr(label))).clicked() {
                        *chosen = Some(Command::Toggle(*flag));
                    }
                }
                Sub(label, inner) if opened_out => {
                    ui.label(RichText::new(crate::i18n::tr(label)).color(theme::text_dim()));
                    ui.indent(*label, |ui| self.draw_entries(ui, inner, true, chosen, picks));
                }
                // Laid open for a picture, a sub-menu shows as it does in a menu that's been clicked: a name and an arrow.
                Sub(label, _) if self.drawing_open => {
                    ui.add(Button::new(crate::i18n::tr(label)).shortcut_text(RichText::new(crate::i18n::tr("\u{203A}")).color(theme::text_faint())).min_size(egui::vec2(MENU_WIDTH - 16.0, 0.0)));
                }
                Sub(label, inner) => {
                    crate::fit::menu_button(ui, crate::i18n::tr(label), |ui| {
                        ui.set_min_width(MENU_WIDTH);
                        self.draw_entries(ui, inner, false, chosen, picks);
                    });
                }
                Entry::Live(label, live) if opened_out || matches!(*live, Live::TabSize | Live::TagList) => self.live_entries(ui, *live, label, chosen, picks),
                Entry::Live(label, _) if self.drawing_open => {
                    ui.add(Button::new(crate::i18n::tr(label)).shortcut_text(RichText::new(crate::i18n::tr("\u{203A}")).color(theme::text_faint())).min_size(egui::vec2(MENU_WIDTH - 16.0, 0.0)));
                }
                // The recent files' menu has tabs at its top, which change what it lists without closing it.
                Entry::Live(label, live @ Live::RecentFiles) => {
                    crate::fit::menu_button_kept_open(ui, crate::i18n::tr(label), |ui| {
                        ui.set_min_width(MENU_WIDTH);
                        self.live_entries(ui, *live, label, chosen, picks);
                    });
                }
                Entry::Live(label, live) => {
                    crate::fit::menu_button(ui, crate::i18n::tr(label), |ui| {
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

    /// A list of the menus' entries in one of the ribbon's drop-downs, drawn as the menus draw it, with what's
    /// picked from it done. A list that changes, such as the recent files, is drawn in place.
    pub(super) fn drop_down_list(&mut self, ui: &mut Ui, entries: &[Entry]) -> Option<Command> {
        let mut chosen = None;
        let mut picks = Vec::new();
        ui.set_min_width(MENU_WIDTH);
        match entries {
            [Entry::Live(label, live)] => self.live_entries(ui, *live, label, &mut chosen, &mut picks),
            _ => self.draw_entries(ui, entries, false, &mut chosen, &mut picks),
        }
        self.carry_out(picks);
        chosen
    }

    fn live_entries(&mut self, ui: &mut Ui, live: Live, label: &str, chosen: &mut Option<Command>, picks: &mut Vec<Pick>) {
        let empty = |ui: &mut Ui, words: &str| {
            ui.label(RichText::new(words).color(theme::text_faint()));
        };
        let name_of = |path: &Path| path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
        match live {
            Live::RecentFiles => {
                // Two tabs: the recent projects, shown first each time the menu opens, and the recent files, grouped
                // by the project each was in.
                let mut tab = self.recent_lists.menu_tab(ui.ctx().cumulative_pass_nr());
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut tab, super::recent::Tab::Projects, crate::i18n::tr("Projects"));
                    ui.selectable_value(&mut tab, super::recent::Tab::Files, crate::i18n::tr("Files"));
                });
                self.recent_lists.choose_menu_tab(tab);
                ui.separator();
                if tab == super::recent::Tab::Projects {
                    self.recent_project_entries(ui, picks);
                    return;
                }
                if self.panels.recent.is_empty() {
                    empty(ui, crate::i18n::tr("Files you open will be listed here."));
                }
                let groups = super::recent::grouped(&self.panels.recent[..self.panels.recent.len().min(super::recent::MENU_FILES)], &self.recent_in);
                for group in &groups {
                    let heading = self.recent_lists.group_name(group);
                    ui.label(RichText::new(heading).size(11.5).color(theme::text_faint()));
                    for path in &group.files {
                        let folder = path.parent().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                        let button = Button::new(name_of(path)).shortcut_text(RichText::new(folder).color(theme::text_faint()));
                        if ui.add(button).on_hover_text(path.display().to_string()).clicked() {
                            picks.push(Pick::RecentFile(path.clone(), group.project.clone()));
                            ui.close();
                        }
                    }
                }
                if !self.panels.recent.is_empty() {
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button(crate::i18n::tr("Clean up")).on_hover_text(crate::i18n::tr("Take files that are no longer there off the list")).clicked() {
                            picks.push(Pick::Command(Command::CleanRecent));
                            ui.close();
                        }
                        ui.label(RichText::new(crate::i18n::tr("|")).color(theme::text_faint()));
                        if ui.button(crate::i18n::tr("Clear list")).on_hover_text(crate::i18n::tr("Empty the list")).clicked() {
                            picks.push(Pick::Command(Command::ClearRecent));
                            ui.close();
                        }
                    });
                }
            }
            Live::Tasks => {
                let tasks = self.tasks_here();
                if tasks.is_empty() {
                    empty(ui, crate::i18n::tr("No tasks here: none of Cargo.toml, a Makefile, package.json, go.mod or CMakeLists.txt above the file, and no project tasks."));
                }
                let folder = self.task_folder();
                for (index, task) in tasks.iter().enumerate() {
                    // A package.json script can also be run under the debugger, from the button beside it.
                    let debuggable = folder.as_deref().is_some_and(|f| crate::node_tools::script_of(f, &task.command).is_some());
                    ui.horizontal(|ui| {
                        if ui.add(Button::new(task.name.as_str()).shortcut_text(RichText::new(task.kind.label()).color(theme::text_faint()))).on_hover_text(&task.command).clicked() {
                            picks.push(Pick::Command(Command::RunTask(index)));
                            ui.close();
                        }
                        if debuggable && ui.button(crate::i18n::tr("Debug")).on_hover_text(crate::i18n::tr("Run this script under the debugger (js-debug)")).clicked() {
                            picks.push(Pick::Command(Command::DebugTask(index)));
                            ui.close();
                        }
                    });
                }
            }
            Live::NewAs => {
                // The kinds chosen last, first, then a line; they may have been taken off the list since.
                let recent: Vec<usize> = self.recent_new_kinds.iter().filter_map(|name| self.new_kinds.iter().position(|kind| kind.name == *name)).collect();
                for &index in &recent {
                    let kind = &self.new_kinds[index];
                    let ending = if kind.ending.is_empty() { String::new() } else { format!(".{}", kind.ending) };
                    if ui.add(Button::new(kind.name.as_str()).shortcut_text(RichText::new(ending).color(theme::text_faint()))).clicked() {
                        picks.push(Pick::Command(Command::NewAs(index)));
                        ui.close();
                    }
                }
                if !recent.is_empty() {
                    ui.separator();
                }
                // A sub-menu for each kind of machine, and the modern languages, so the list stays short.
                for group in super::new_kinds::GROUPS {
                    let members: Vec<usize> = self.new_kinds.iter().enumerate().filter(|(_, kind)| kind.group() == group).map(|(index, _)| index).collect();
                    if members.is_empty() {
                        continue;
                    }
                    crate::fit::menu_button(ui, crate::i18n::tr(group), |ui| {
                        for index in members {
                            let kind = &self.new_kinds[index];
                            let ending = if kind.ending.is_empty() { String::new() } else { format!(".{}", kind.ending) };
                            if ui.add(Button::new(kind.name.as_str()).shortcut_text(RichText::new(ending).color(theme::text_faint()))).clicked() {
                                picks.push(Pick::Command(Command::NewAs(index)));
                                ui.close();
                            }
                        }
                        // A C program for the Amiga is usually a project (makefiles for amiga-gcc, vbcc and SAS/C),
                        // so it's offered beside the single files.
                        if group == "Amiga" {
                            ui.separator();
                            if ui.button(crate::i18n::tr("New Amiga C project\u{2026}")).clicked() {
                                picks.push(Pick::Command(Command::AmigaNewProject));
                                ui.close();
                            }
                        }
                    });
                }
                // The project's own templates, under its name.
                let own = self.project_templates();
                if !own.is_empty()
                    && let Some(project) = &self.project
                {
                    crate::fit::menu_button(ui, project.name.as_str(), |ui| {
                        for (index, template) in own.iter().enumerate() {
                            let label = if template.folder { format!("{}/", template.name) } else { template.name.clone() };
                            if ui.button(label).clicked() {
                                picks.push(Pick::Command(Command::NewFromTemplate(index)));
                                ui.close();
                            }
                        }
                    });
                }
                ui.separator();
                if ui.button(crate::i18n::tr("Change this list\u{2026}")).clicked() {
                    picks.push(Pick::Command(Command::EditNewKinds));
                    ui.close();
                }
            }
            Live::RecentProjects => self.recent_project_entries(ui, picks),
            Live::ProjectAutosave => {
                // This project's own choice, which beats the settings'; None leaves it to them.
                let chosen = self.project.as_ref().and_then(|project| project.autosave);
                let mut choices: Vec<(Option<crate::settings::AutosaveChoice>, &str)> = vec![(None, "As in the settings"), (Some(crate::settings::AutosaveChoice::Off), "Not by themselves")];
                choices.extend(crate::settings::AutosaveWhen::ALL.into_iter().map(|when| (Some(crate::settings::AutosaveChoice::When(when)), when.label())));
                for (choice, label) in choices {
                    if ui.add_enabled(self.project.is_some(), Button::selectable(chosen == choice, crate::i18n::tr(label))).clicked() {
                        picks.push(Pick::Command(Command::ProjectAutosave(choice)));
                        ui.close();
                    }
                }
            }
            Live::ClaudeCode => {
                // Only there when a Claude Code in the terminal is connected to be told.
                if self.claude_connected() {
                    ui.separator();
                    if ui.add_enabled(!self.no_file(), Button::new(crate::i18n::tr("Mention in Claude Code"))).on_hover_text(crate::i18n::tr("Puts this file, and the lines selected, into Claude Code's prompt")).clicked() {
                        picks.push(Pick::Command(Command::MentionInClaude));
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
                    empty(ui, crate::i18n::tr("For documents no rule picks a theme for. Themes and rules sets the rules."));
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
                let own = self.own_theme(self.active);
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
                    empty(ui, crate::i18n::tr("Devices you set up are listed here. Manage devices adds one: a card, a folder, a server, a Spectrum Next or an Ultimate 64."));
                }
                let usable = !self.no_file() && self.docs[self.active].browser.is_none();
                for number in 0..self.sends.devices.len() {
                    let device = self.sends.devices[number].clone();
                    let absent = matches!(&device.place, crate::deploy::Place::Card { label, .. } if self.sends.card_present(label) == Some(false));
                    let label = if absent { crate::i18n::tf!("{}  (not plugged in)", device.name) } else { device.name.clone() };
                    if ui.add_enabled(usable, Button::new(label)).on_hover_text(device.place.describe()).clicked() {
                        picks.push(Pick::Device(number));
                        ui.close();
                    }
                }
            }
            Live::Machines => {
                // What the file in front would run as, so each machine says whether it can take it.
                let runnable = if self.no_file() { None } else { self.runnable_quietly().ok().flatten() };
                let mut targets = vec![(super::run_host::Target::BuiltIn, super::run_host::BUILT_IN_NAME.to_string(), Pick::RunOn(None)), (super::run_host::Target::BuiltIn2600, super::run_host::BUILT_IN_2600_NAME.to_string(), Pick::Command(Command::RunOnBuiltIn2600)), (super::run_host::Target::BuiltInVectrex, super::run_host::BUILT_IN_VECTREX_NAME.to_string(), Pick::Command(Command::RunOnBuiltInVectrex))];
                // The file in front's machine, in Limoniter's window, when it has a libretro core.
                if let Some(machine) = runnable.as_ref().map(|run| run.machine).filter(|m| crate::retro::core_for(*m).is_some()) {
                    targets.push((super::run_host::Target::Retro(machine), crate::retro::target_name(machine), Pick::Command(Command::RunInWindow(machine))));
                }
                targets.extend((0..self.sends.devices.len()).map(|n| (super::run_host::Target::Device(n), self.sends.devices[n].name.clone(), Pick::RunOn(Some(n)))));
                for (target, name, pick) in targets {
                    let suits = runnable.as_ref().map(|run| self.suits(target, run));
                    let button = ui.add_enabled(!matches!(suits, Some(Err(_))), Button::new(name));
                    let button = match &suits {
                        // A machine in Limoniter's window that lacks only a download can be installed from F9's box.
                        Some(Err(why)) if matches!(target, super::run_host::Target::Retro(_)) && runnable.as_ref().and_then(|run| crate::retro::missing(run.machine, run)).is_some_and(|missing| missing.button().is_some()) => button.on_disabled_hover_text(crate::i18n::tf!("{why} F9's Run on machine box can install it.")),
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
                    empty(ui, crate::i18n::tr("Add-ons that are on list their commands here. Manage add-ons turns them on."));
                }
                for (number, listed) in commands.iter().enumerate() {
                    if ui.button(listed.title.as_str()).on_hover_text(crate::i18n::tf!("From {}", listed.addon)).clicked() {
                        picks.push(Pick::AddOn(number));
                        ui.close();
                    }
                }
            }
            Live::Snippets => {
                if self.snippets.is_empty() {
                    empty(ui, crate::i18n::tr("Snippets you make will be listed here."));
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
            Live::Layouts | Live::DeleteLayouts => {
                if self.layouts.is_empty() {
                    empty(ui, crate::i18n::tr("Layouts you save will be listed here."));
                }
                for (index, layout) in self.layouts.iter().enumerate() {
                    if ui.button(layout.name.as_str()).clicked() {
                        picks.push(if live == Live::Layouts { Pick::UseLayout(index) } else { Pick::DeleteLayout(index) });
                        ui.close();
                    }
                }
            }
            Live::SurroundSnippets => {
                // The snippets that take in the selection: those with $SELECTION in them.
                let usable = !self.no_file() && !self.docs[self.active].hex_mode;
                let mut any = false;
                for (index, snippet) in self.snippets.iter().enumerate().filter(|(_, s)| s.body.contains("$SELECTION")) {
                    any = true;
                    let preview: String = snippet.body.chars().take(200).collect();
                    if ui.add_enabled(usable, Button::new(snippet.name.as_str())).on_hover_text(preview).clicked() {
                        picks.push(Pick::Snippet(index));
                        ui.close();
                    }
                }
                if !any {
                    empty(ui, crate::i18n::tr("Snippets with $SELECTION in them will be listed here."));
                }
            }
            Live::TagList => {
                let usable = !self.no_file() && !self.docs[self.active].hex_mode;
                for tag in crate::own_tags::all() {
                    let mut job = egui::text::LayoutJob::default();
                    RichText::new(crate::i18n::tr("\u{25A0}  ")).color(tag.colour).append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                    RichText::new(tag.name.as_str()).append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                    let mut button = Button::new(job);
                    if !tag.key.is_empty() {
                        button = button.shortcut_text(RichText::new(shown_shortcut(&tag.key)).color(theme::text_faint()));
                    }
                    let response = ui.add_enabled(usable, button);
                    let response = if tag.project { response.on_hover_text(crate::i18n::tr("Kept with this project")) } else { response };
                    if response.clicked() {
                        picks.push(Pick::Command(Command::Tag(tag.id)));
                        ui.close();
                    }
                }
            }
            Live::Tags => {
                let text = &self.docs[self.active].text;
                let tags = if self.docs[self.active].hex_mode || self.no_file() { Vec::new() } else { text.tags() };
                if tags.is_empty() {
                    empty(ui, crate::i18n::tr("Sections you tag with a colour will be listed here."));
                }
                // What's typed narrows the list to the tags whose name has it, or the lines that do.
                let filter_key = "tag-go-to-filter";
                let filter_id = ui.scope_id().with(filter_key);
                let mut typed = ui.data_mut(|data| data.get_temp::<String>(filter_id)).unwrap_or_default();
                if !tags.is_empty() {
                    let field = ui.add(egui::TextEdit::singleline(&mut typed).hint_text(crate::i18n::tr("Type a tag's name")).desired_width(MENU_WIDTH - 24.0));
                    if !field.has_focus() && typed.is_empty() && ui.memory(|m| m.focused().is_none()) {
                        field.request_focus();
                    }
                    ui.data_mut(|data| data.insert_temp(filter_id, typed.clone()));
                }
                let wanted = typed.trim().to_lowercase();
                let named: Vec<u8> = crate::own_tags::matching(&typed).into_iter().map(|t| t.id).collect();
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    let mut listed = 0;
                    for (first, last, colour) in tags {
                        let words: String = text.format.readable_text(&text.line(first)).trim().chars().take(48).collect();
                        let name = crate::own_tags::name(colour);
                        if !wanted.is_empty() && !named.contains(&colour) && !words.to_lowercase().contains(&wanted) {
                            continue;
                        }
                        listed += 1;
                        let mut job = egui::text::LayoutJob::default();
                        RichText::new(crate::i18n::tr("\u{25A0}  ")).color(crate::own_tags::colour(colour)).append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                        RichText::new(crate::i18n::tf!("{}  ", name)).strong().append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                        RichText::new(crate::i18n::tf!("{}  ", first + 1)).color(theme::text_faint()).append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                        RichText::new(if words.is_empty() { crate::i18n::tr("(blank line)").to_string() } else { words }).append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                        if ui.button(job).on_hover_text(crate::i18n::tf!("Lines {} to {}", first + 1, last + 1)).clicked() {
                            picks.push(Pick::Line(first));
                            ui.close();
                        }
                    }
                    if listed == 0 && !wanted.is_empty() {
                        empty(ui, crate::i18n::tr("No tag has that name."));
                    }
                });
            }
            Live::Bookmarks => {
                let text = &self.docs[self.active].text;
                let marks = if self.docs[self.active].hex_mode || self.no_file() { Vec::new() } else { text.bookmarks() };
                if marks.is_empty() {
                    empty(ui, crate::i18n::tr("Lines you bookmark (Ctrl+F2) will be listed here."));
                }
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    for line in marks.into_iter().take(500) {
                        let words: String = text.format.readable_text(&text.line(line)).trim().chars().take(48).collect();
                        let mut job = egui::text::LayoutJob::default();
                        RichText::new(crate::i18n::tf!("{}  ", line + 1)).color(theme::text_faint()).append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                        RichText::new(if words.is_empty() { crate::i18n::tr("(blank line)").to_string() } else { words }).append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                        if ui.button(job).clicked() {
                            picks.push(Pick::Line(line));
                            ui.close();
                        }
                    }
                });
            }
            Live::TabSize => {
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr(&*label));
                    ui.add(egui::DragValue::new(&mut self.view.tab_size).range(text_buffer::MIN_TAB_SIZE..=text_buffer::MAX_TAB_SIZE)).on_hover_text(crate::i18n::tr("Columns from one tab stop to the next, for all open files"));
                });
            }
        }
        let _ = chosen;
    }

    /// The recent projects, each by its name, with a folder opened as a project showing its folder faintly.
    fn recent_project_entries(&mut self, ui: &mut Ui, picks: &mut Vec<Pick>) {
        if self.recent_projects.is_empty() {
            ui.label(RichText::new(crate::i18n::tr("Projects you open will be listed here.")).color(theme::text_faint()));
        }
        for path in self.recent_projects.clone() {
            let (name, place, folder) = self.recent_lists.shown_as(&path);
            let faint = if folder { super::recent::shortened(&place, 40) } else { String::new() };
            if ui.add(Button::new(name).shortcut_text(RichText::new(faint).color(theme::text_faint()))).on_hover_text(place).clicked() {
                picks.push(Pick::Project(path));
                ui.close();
            }
        }
    }

    fn carry_out(&mut self, picks: Vec<Pick>) {
        for pick in picks {
            match pick {
                Pick::RecentFile(path, project) => self.open_recent_file(path, project),
                Pick::Project(path) => self.open_project(&path),
                Pick::Snippet(index) => self.insert_snippet(index),
                Pick::UseLayout(index) => self.apply_layout(index),
                Pick::DeleteLayout(index) => {
                    if index < self.layouts.len() {
                        self.layouts.remove(index);
                    }
                }
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
                Pick::ThisDocumentTheme(name) => self.set_file_theme(self.active, name),
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
            if self.view.ribbon {
                // The ribbon's tabs take the menus' place.
                self.ribbon_tab_bar(ui, &mut chosen);
            } else {
                for (number, (title, entries)) in MENUS.iter().enumerate() {
                    let menu = crate::fit::menu_button(ui, crate::i18n::tr(title), |ui| {
                        ui.set_min_width(MENU_WIDTH);
                        self.draw_entries(ui, entries, false, &mut chosen, &mut picks);
                    });
                    if number == 0 {
                        ui.ctx().data_mut(|d| d.insert_temp(super::access_host::first_menu_key(), menu.response.id));
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if command_box(ui).clicked() {
                    chosen = Some(Command::CommandPalette);
                }
                ui.add_space(2.0);
                if self.ribbon_switch(ui) {
                    chosen = Some(Command::Toggle(Flag::Ribbon));
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
                    crate::fit::menu_body(ui, |ui| self.draw_entries(ui, entries, opened_out, &mut chosen, &mut picks));
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
        all.extend(self.sends.devices.iter().enumerate().map(|(number, device)| Found { path: crate::i18n::tf!("File \u{203A} Send to device \u{203A} {}", device.name), shortcut: "", command: Command::SendTo(number) }));
        all.push(Found { path: crate::i18n::tf!("File \u{203A} Run on \u{203A} {}", super::run_host::BUILT_IN_NAME), shortcut: "", command: Command::RunOnBuiltIn });
        all.push(Found { path: crate::i18n::tf!("File \u{203A} Run on \u{203A} {}", super::run_host::BUILT_IN_2600_NAME), shortcut: "", command: Command::RunOnBuiltIn2600 });
        all.push(Found { path: crate::i18n::tf!("File \u{203A} Run on \u{203A} {}", super::run_host::BUILT_IN_VECTREX_NAME), shortcut: "", command: Command::RunOnBuiltInVectrex });
        all.extend(self.sends.devices.iter().enumerate().map(|(number, device)| Found { path: crate::i18n::tf!("File \u{203A} Run on \u{203A} {}", device.name), shortcut: "", command: Command::RunOn(number) }));
        all.extend(self.addons.commands().into_iter().enumerate().map(|(number, listed)| Found { path: crate::i18n::tf!("Tools \u{203A} Add-ons \u{203A} {} \u{203A} {}", listed.addon, listed.title), shortcut: "", command: Command::AddOn(number) }));
        let mut found = hidden_commands(&query);
        found.extend(matching(&query, all));
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
        let mut closed = false;
        // As wide as it was last left: its right and left edges drag to widen it.
        let width = self.palette_width.clamp(MIN_PALETTE_WIDTH, MAX_PALETTE_WIDTH);
        let shown = crate::fit::Dialog::new(crate::i18n::tr("Find a command")).title_bar(false).collapsible(false).resizable([true, false]).default_width(width).min_width(MIN_PALETTE_WIDTH).max_width(MAX_PALETTE_WIDTH).anchor(egui::Align2::CENTER_TOP, [0.0, 64.0]).show(ctx, |ui| {
            // The box has no title bar, so it has a close button of its own beside the field.
            let (field, close) = ui
                .horizontal(|ui| {
                    let field = ui.add(egui::TextEdit::singleline(&mut state.query).hint_text(crate::i18n::tr("Type what you want to do, such as \u{201C}sort\u{201D} or \u{201C}wrap\u{201D}")).desired_width(ui.available_width() - 30.0).margin(egui::vec2(10.0, 8.0)).font(egui::FontId::proportional(16.0)));
                    let close = crate::access::called(ui.add(egui::Button::new(RichText::new(crate::i18n::tr("\u{D7}")).size(16.0).color(theme::text_dim())).frame(false)), "Close").on_hover_text(crate::i18n::tr("Close (Esc)")).clicked();
                    (field, close)
                })
                .inner;
            closed |= close;
            if state.focus {
                field.request_focus();
                state.focus = false;
            }
            if field.changed() {
                state.selected = 0;
            }
            let field_id = field.id;
            ui.add_space(6.0);
            if found.is_empty() {
                ui.label(RichText::new(crate::i18n::tr("Nothing matches that. Try another word.")).color(theme::text_faint()));
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
                    ui.painter().text(egui::pos2(rect.max.x - 12.0, rect.center().y), egui::Align2::RIGHT_CENTER, &right, egui::FontId::proportional(12.0), theme::text_faint());
                    // Read out as an option in a list, the one chosen being the field's active one, so a screen reader
                    // follows the arrows while the typing stays in the field.
                    let mut spoken = name.to_string();
                    if !place.is_empty() {
                        spoken += &format!(", in {}", place.replace(" \u{203A} ", ", "));
                    }
                    if !right.is_empty() {
                        spoken += &format!(", {right}");
                    }
                    crate::access::chosen(&response, egui::Role::ListBoxOption, here, &spoken);
                    if !usable[index] {
                        ui.ctx().accesskit_node_builder(response.id, |node| node.set_disabled());
                    }
                    if here {
                        ui.ctx().accesskit_node_builder(field_id, |node| node.set_active_descendant(response.id.accesskit_id()));
                    }
                    if response.clicked() && usable[index] {
                        run = Some(each.command);
                    }
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new(crate::i18n::tr("\u{2191} \u{2193} to choose, Enter to do it, Esc to close")).color(theme::text_faint()).size(11.5));
        });
        if let Some(shown) = shown {
            let now = shown.response.rect.width();
            if (now - self.palette_width).abs() > 1.0 && now >= MIN_PALETTE_WIDTH {
                // Kept with the settings when they're next saved.
                self.palette_width = now;
            }
        }
        if enter && let Some(index) = found.get(state.selected).map(|_| state.selected) && usable[index] {
            run = Some(found[index].command);
        }
        if escape || closed || run.is_some() {
            self.close_dialog();
        }
        self.dialog_was_palette = run.is_some();
        self.pending_command = run;
        true
    }

    pub(super) fn about(&mut self) {
        // Beside the program from the Windows installer; in share/doc/limoniter beside bin/ from the Linux packages.
        let exe = std::env::current_exe().ok();
        let folder = exe.as_deref().and_then(Path::parent);
        let beside = folder.map(|folder| folder.join("THIRD-PARTY-NOTICES.txt"));
        let shared = folder.and_then(Path::parent).map(|up| up.join("share/doc/limoniter/THIRD-PARTY-NOTICES.txt"));
        let notices = beside.into_iter().chain(shared).find(|path| path.is_file());
        self.dialog = Dialog::About { notices };
    }
}

/// Looks like a search box, and opens the command box.
fn command_box(ui: &mut Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(250.0, 24.0), egui::Sense::click());
    let back = if response.hovered() { theme::hover() } else { theme::field() };
    ui.painter().rect(rect, 7.0, back, egui::Stroke::new(1.0, theme::border()), egui::StrokeKind::Inside);
    ui.painter().text(egui::pos2(rect.min.x + 10.0, rect.center().y), egui::Align2::LEFT_CENTER, crate::i18n::tr("Find a command\u{2026}"), egui::FontId::proportional(13.0), theme::text_faint());
    ui.painter().text(egui::pos2(rect.max.x - 10.0, rect.center().y), egui::Align2::RIGHT_CENTER, "F1", egui::FontId::proportional(12.0), theme::text_faint());
    crate::access::button(&response, "Find a command (F1)");
    response.on_hover_text(crate::i18n::tr("Find any command by what it does"))
}

/// The command box's narrowest and widest.
const MIN_PALETTE_WIDTH: f32 = 420.0;
const MAX_PALETTE_WIDTH: f32 = 1400.0;

#[cfg(test)]
mod mac_tests {
    #[test]
    fn shortcuts_are_shown_the_macs_way_there() {
        for (pc, mac) in [("Ctrl+S", "\u{2318}S"), ("Ctrl+Shift+S", "\u{21E7}\u{2318}S"), ("Ctrl+Alt+H", "\u{2325}\u{2318}H"), ("Alt+U", "\u{2303}\u{2325}U"), ("Alt+Left", "\u{2318}["), ("Ctrl+Tab", "\u{2303}Tab"), ("F6", "F6"), ("Shift+F3", "\u{21E7}F3")] {
            assert_eq!(super::mac_shortcut(pc), mac, "{pc}");
        }
    }
}

