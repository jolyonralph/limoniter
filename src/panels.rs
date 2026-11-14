//! The docked panels switched on from the Layout tab: file properties, JSON and XML managers,
//! Character table, clipboard history, encoding preview, file explorer, open files and bookmarks.

use crate::char_table::{self, TableState};
use crate::charset::Charset;
use crate::codec::{self, Eol, TextFormat};
use crate::hex_buffer::HexBuffer;
use crate::text_buffer::{Counts, TextBuffer, count_lines};
use crate::theme;
use eframe::egui::{self, Align2, Color32, FontId, Panel, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Files larger than this aren't parsed by the JSON and XML managers or counted by File properties.
const MAX_PARSE_BYTES: usize = 32 * 1024 * 1024;
/// While someone is typing, the panels catch up at most this often.
const REFRESH_SECONDS: f64 = 0.5;
const MAX_CLIPBOARD_ITEMS: usize = 50;
const MAX_CLIPBOARD_ITEM_BYTES: usize = 1024 * 1024;
const CLIPBOARD_POLL_SECONDS: f64 = 1.0;

/// What a panel needs to know about the document being shown.
pub struct DocView<'a> {
    pub id: u64,
    pub text: &'a TextBuffer,
    pub hex: &'a HexBuffer,
    pub format: &'a TextFormat,
    pub path: Option<&'a Path>,
    pub hex_mode: bool,
    pub name: String,
    pub language: String,
    /// There's no file open, and this is only standing in.
    pub no_file: bool,
    /// The grammar this file is being read with, if it has one.
    pub syntax: Option<usize>,
    /// For a tab browsing a disk image or archive: what it says about itself, and the file chosen in it.
    pub image: Option<ImageView<'a>>,
}

/// What a browser tab shows File properties: the image's own sections, a picture it carries, and the chosen file.
pub struct ImageView<'a> {
    /// Where in the image the browser is, which also tells one picture from another.
    pub place: String,
    pub sections: &'a [crate::media::InfoSection],
    /// A PNG.
    pub picture: Option<&'a [u8]>,
    pub selected: Option<&'a crate::media::Entry>,
}

impl DocView<'_> {
    fn version(&self) -> u64 {
        if self.hex_mode { self.hex.version() } else { self.text.version() }
    }

    /// The document's text, unless it's in hex mode or too large to hold in memory.
    fn whole_text(&self) -> Result<String, String> {
        if self.hex_mode {
            return Err("Switch to text mode to read this file here.".into());
        }
        if self.text.is_disk_backed() {
            return Err("This file is too large to read here. It is being kept on disk.".into());
        }
        let lines = self.text.all_lines();
        let size: usize = lines.iter().map(|l| l.len() + 1).sum();
        if size > MAX_PARSE_BYTES {
            return Err("This file is too large to read here.".into());
        }
        Ok(lines.iter().map(|l| &**l).collect::<Vec<_>>().join("\n"))
    }
}

/// One open tab, for the Open Files list.
pub struct TabInfo {
    pub title: String,
    pub path: Option<PathBuf>,
}

/// What someone did in the Edit history panel.
pub enum HistoryAction {
    Jump(crate::undo_tree::NodeId),
    Compare(crate::undo_tree::NodeId),
    Forget,
}

/// When a state was reached: the time, with the date as well if it wasn't today.
fn history_time(when: u64) -> String {
    let Some(time) = i64::try_from(when).ok().and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0)) else {
        return String::new();
    };
    let time = time.with_timezone(&chrono::Local);
    let same_day = time.date_naive() == chrono::Local::now().date_naive();
    time.format(if same_day { "%H:%M:%S" } else { "%d %b %H:%M" }).to_string()
}

const HISTORY_ROW: f32 = 22.0;
const HISTORY_LANE: f32 = 14.0;
/// However many branches there are, the words stay in reach.
const HISTORY_MOST_LANES: usize = 12;

/// Draws the tree of states, newest at the top, each branch going down its own column.
fn history_panel(ui: &mut Ui, rows: &[crate::undo_tree::Row], hex: bool, actions: &mut Vec<HistoryAction>) {
    if hex {
        ui.label(RichText::new("The history is for text. In hex mode, Undo and Redo work as a list.").color(theme::text_faint()).size(12.5));
        return;
    }
    if rows.is_empty() {
        ui.label(RichText::new("Nothing has been changed yet. Each change will be listed here, and undoing one and typing something else starts a new branch, with nothing lost.").color(theme::text_faint()).size(12.5));
        return;
    }
    egui::Panel::bottom("history-buttons").frame(egui::Frame::new()).show(ui, |ui| {
        ui.add_space(4.0);
        ui.label(RichText::new("Click a state to go to it. Right-click to compare it. Where you are now stays in the list.").color(theme::text_faint()).size(12.0));
        ui.add_space(2.0);
        if ui.button("Forget this history...").on_hover_text("Lets go of every earlier state of this file. The text stays as it is").clicked() {
            actions.push(HistoryAction::Forget);
        }
    });
    let lanes = rows.iter().map(|row| row.lane + 1).max().unwrap_or(1).min(HISTORY_MOST_LANES);
    let graph = lanes as f32 * HISTORY_LANE + 6.0;
    egui::ScrollArea::both().id_salt("history-scroll").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let width = ui.available_width().max(graph + 220.0);
        let top = ui.cursor().min.y;
        let centre = |lane: usize, row: usize, left: f32| pos2(left + 6.0 + lane.min(HISTORY_MOST_LANES - 1) as f32 * HISTORY_LANE, top + row as f32 * HISTORY_ROW + HISTORY_ROW / 2.0);
        for (index, row) in rows.iter().enumerate() {
            let (rect, response) = ui.allocate_exact_size(vec2(width, HISTORY_ROW), Sense::click());
            let painter = ui.painter();
            if row.here {
                painter.rect_filled(rect, 2.0, theme::selected());
            } else if response.hovered() {
                painter.rect_filled(rect, 2.0, theme::hover());
            }
            let colour = if row.on_path { theme::accent() } else { theme::text_faint() };
            let dot = centre(row.lane, index, rect.min.x);
            if let Some(parent) = row.parent_row.and_then(|place| rows.get(place).map(|above| (above, place))) {
                let target = centre(parent.0.lane, parent.1, rect.min.x);
                // Straight down its own column, then across to what it came from.
                let bend = pos2(dot.x, target.y - HISTORY_ROW / 2.0);
                let stroke = Stroke::new(1.5, colour);
                painter.line_segment([dot, bend], stroke);
                painter.line_segment([bend, target], stroke);
            }
            painter.circle_filled(dot, if row.here { 5.0 } else { 3.5 }, colour);
            let words = match row.id {
                Some(_) => format!("{}  {}", history_time(row.when), row.label),
                None => row.label.clone(),
            };
            let text_colour = if row.on_path { theme::text() } else { theme::text_dim() };
            painter.text(pos2(rect.min.x + graph, rect.center().y), Align2::LEFT_CENTER, words, FontId::proportional(12.5), text_colour);
            let Some(id) = row.id else {
                continue;
            };
            if response.clicked() {
                actions.push(HistoryAction::Jump(id));
            }
            response.context_menu(|ui| {
                if ui.button("Go to this state").clicked() {
                    actions.push(HistoryAction::Jump(id));
                    ui.close();
                }
                if ui.button("Compare with the text now").clicked() {
                    actions.push(HistoryAction::Compare(id));
                    ui.close();
                }
            });
        }
    });
}

/// What a panel asks the window to do.
pub enum Action {
    /// A table entry: its exact byte if the set is the document's own.
    InsertEntry { set: &'static Charset, table: usize, byte: u8 },
    InsertCharacter(char),
    /// Exact bytes, as one undoable action.
    InsertBytes(Vec<u8>),
    AskInsertByte,
    Paste(String),
    OpenPath(PathBuf),
    OpenDialog,
    GoToLine(usize),
    SwitchTab(usize),
    CopyText(String),
    /// Select these bytes in hex mode, from `start` up to `end`.
    SelectBytes { start: usize, end: usize },
    /// Switch the document to hex mode.
    ViewHex,
    /// Text to go either side of the selection.
    WrapTag(String, String),
}

/// A piece of text to put in, or to put either side of a selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    pub before: String,
    pub after: String,
}

/// The tags everyone starts with, in groups.
const BUILT_IN_TAGS: [(&str, &[(&str, &str, &str)]); 2] = [
    (
        "HTML",
        &[
            ("Bold", "<b>", "</b>"),
            ("Italic", "<i>", "</i>"),
            ("Paragraph", "<p>", "</p>"),
            ("Division", "<div>", "</div>"),
            ("Span", "<span>", "</span>"),
            ("Heading 1", "<h1>", "</h1>"),
            ("Heading 2", "<h2>", "</h2>"),
            ("Link", "<a href=\"\">", "</a>"),
            ("List", "<ul>\n", "\n</ul>"),
            ("List item", "<li>", "</li>"),
            ("Code", "<code>", "</code>"),
            ("Comment", "<!-- ", " -->"),
        ],
    ),
    (
        "Markdown",
        &[
            ("Bold", "**", "**"),
            ("Italic", "*", "*"),
            ("Strikethrough", "~~", "~~"),
            ("Code", "`", "`"),
            ("Code block", "```\n", "\n```"),
            ("Link", "[", "]()"),
            ("Image", "![", "]()"),
            ("Heading 1", "# ", ""),
            ("Heading 2", "## ", ""),
            ("Quote", "> ", ""),
            ("Bullet", "- ", ""),
        ],
    ),
];

const MY_TAGS: &str = "My tags";

/// The Tags panel's state.
#[derive(Default)]
struct TagsPanel {
    group: String,
    name: String,
    before: String,
    after: String,
}

/// Works out a file's functions on another thread, so that a long file can't hold up typing.
struct FunctionJob {
    key: (u64, u64),
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    result: std::sync::Arc<std::sync::Mutex<Option<Vec<(usize, String)>>>>,
}

/// The Functions panel's state.
#[derive(Default)]
struct FunctionsPanel {
    /// The document and version these came from.
    key: Option<(u64, u64)>,
    items: Vec<(usize, String)>,
    job: Option<FunctionJob>,
    started: f64,
    filter: String,
}

#[derive(Default)]
pub struct Panels {
    pub file_properties: bool,
    pub json_manager: bool,
    pub xml_manager: bool,
    pub ascii_table: bool,
    pub clipboard_history: bool,
    pub encoding_preview: bool,
    pub file_explorer: bool,
    pub open_files: bool,
    pub bookmarks: bool,
    pub file_lists: bool,
    pub functions: bool,
    pub tags: bool,
    pub project: bool,
    pub macros: bool,
    pub scripts: bool,
    pub history: bool,
    pub strings: bool,
    strings_panel: crate::strings_explorer::PanelState,
    macros_panel: crate::scripting::PanelState,
    scripts_panel: crate::scripting::PanelState,
    project_panel: crate::project::PanelState,
    /// Tags added by the person. Kept in the settings file.
    pub custom_tags: Vec<Tag>,
    functions_panel: FunctionsPanel,
    tags_panel: TagsPanel,
    /// Recently opened files, newest first. Kept in the settings file.
    pub recent: Vec<PathBuf>,
    properties: Properties,
    json: TreeManager,
    xml: TreeManager,
    table: TableState,
    pub control_sequences: bool,
    clip_items: Vec<String>,
    clip_last_poll: f64,
    explorer: Explorer,
}

impl Panels {
    /// One "name=value" per line, for the settings file.
    pub fn to_settings(&self) -> String {
        let flags: String = self.flags().iter().map(|(name, value)| format!("{name}={value}\n")).collect();
        let tags: String = self.custom_tags.iter().map(|tag| format!("custom_tag={}\u{1}{}\u{1}{}\n", tag.name, tag.before.replace('\n', "\u{2}"), tag.after.replace('\n', "\u{2}"))).collect();
        flags + tags.as_str()
    }

    fn flags(&self) -> [(&'static str, bool); 18] {
        [
            ("file_properties", self.file_properties),
            ("json_manager", self.json_manager),
            ("xml_manager", self.xml_manager),
            ("ascii_table", self.ascii_table),
            ("control_sequences", self.control_sequences),
            ("clipboard_history", self.clipboard_history),
            ("encoding_preview", self.encoding_preview),
            ("file_explorer", self.file_explorer),
            ("open_files", self.open_files),
            ("bookmarks", self.bookmarks),
            ("file_lists", self.file_lists),
            ("functions", self.functions),
            ("tags", self.tags),
            ("project_panel", self.project),
            ("macros_panel", self.macros),
            ("scripts_panel", self.scripts),
            ("history_panel", self.history),
            ("strings_panel", self.strings),
        ]
    }

    pub fn read_settings(&mut self, text: &str) {
        for line in text.lines() {
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            if name.trim() == "custom_tag" {
                let parts: Vec<&str> = value.split('\u{1}').collect();
                if let [tag_name, before, after] = parts[..] {
                    self.custom_tags.push(Tag { name: tag_name.to_string(), before: before.replace('\u{2}', "\n"), after: after.replace('\u{2}', "\n") });
                }
                continue;
            }
            let Ok(flag) = value.trim().parse::<bool>() else {
                continue;
            };
            match name.trim() {
                "file_properties" => self.file_properties = flag,
                "json_manager" => self.json_manager = flag,
                "xml_manager" => self.xml_manager = flag,
                "ascii_table" => self.ascii_table = flag,
                "control_sequences" => self.control_sequences = flag,
                "clipboard_history" => self.clipboard_history = flag,
                "encoding_preview" => self.encoding_preview = flag,
                "strings_panel" => self.strings = flag,
                "file_explorer" => self.file_explorer = flag,
                "open_files" => self.open_files = flag,
                "bookmarks" => self.bookmarks = flag,
                "file_lists" => self.file_lists = flag,
                "functions" => self.functions = flag,
                "tags" => self.tags = flag,
                "project_panel" => self.project = flag,
                "macros_panel" => self.macros = flag,
                "scripts_panel" => self.scripts = flag,
                "history_panel" => self.history = flag,
                _ => {}
            }
        }
    }

    pub fn hide_all(&mut self) {
        self.file_properties = false;
        self.json_manager = false;
        self.xml_manager = false;
        self.ascii_table = false;
        self.control_sequences = false;
        self.clipboard_history = false;
        self.encoding_preview = false;
        self.file_explorer = false;
        self.open_files = false;
        self.bookmarks = false;
        self.file_lists = false;
        self.functions = false;
        self.tags = false;
        self.project = false;
        self.macros = false;
        self.scripts = false;
        self.history = false;
        self.strings = false;
    }

    fn right_count(&self) -> usize {
        [self.file_properties, self.json_manager, self.xml_manager, self.ascii_table, self.clipboard_history, self.encoding_preview, self.strings]
            .iter()
            .filter(|on| **on)
            .count()
    }

    /// Draws the Project panel, if it's open.
    pub fn show_project(&mut self, ui: &mut Ui, project: Option<&crate::project::Project>, active: Option<&Path>) -> Vec<crate::project::PanelAction> {
        let mut actions = Vec::new();
        if self.project {
            let mut open = true;
            let state = &mut self.project_panel;
            dock(ui, false, "project-panel", "Project", 220.0, &mut open, |ui| actions = crate::project::panel(ui, state, project, active));
            self.project = open;
        }
        actions
    }

    /// Draws the picture of a file's earlier states, if it's open.
    pub fn show_history(&mut self, ui: &mut Ui, rows: &[crate::undo_tree::Row], hex: bool) -> Vec<HistoryAction> {
        let mut actions = Vec::new();
        if self.history {
            let mut open = true;
            dock(ui, false, "history-panel", "Edit history", 300.0, &mut open, |ui| history_panel(ui, rows, hex, &mut actions));
            self.history = open;
        }
        actions
    }

    /// Draws the Macros and Scripts panels, if they're open.
    pub fn show_library(&mut self, ui: &mut Ui, library: &mut crate::scripting::Library) -> Vec<crate::scripting::LibraryAction> {
        use crate::scripting::{self, Kind};
        let mut actions = Vec::new();
        if self.macros || self.scripts {
            library.refresh_if_stale();
        }
        if self.macros {
            let mut open = true;
            let state = &mut self.macros_panel;
            dock(ui, false, "macros-panel", "Macros", 220.0, &mut open, |ui| scripting::panel(ui, Kind::Macro, state, library, &mut actions));
            self.macros = open;
        }
        if self.scripts {
            let mut open = true;
            let state = &mut self.scripts_panel;
            dock(ui, false, "scripts-panel", "Scripts", 220.0, &mut open, |ui| scripting::panel(ui, Kind::Script, state, library, &mut actions));
            self.scripts = open;
        }
        actions
    }

    /// Draws every open panel. The ones on the right go in from the outside in.
    pub fn show(&mut self, ui: &mut Ui, doc: &DocView<'_>, tabs: &[TabInfo], active: usize, actions: &mut Vec<Action>) {
        // First widths in the same proportions as the original, leaving the text room however many are open.
        let whole = ui.available_width();
        let squeeze = (0.56 / (self.right_count() as f32 * 0.112).max(0.56)).min(1.0);
        let share = |part: f32, least: f32, most: f32| (whole * part * squeeze).clamp(least, most);
        let (props_width, tree_width, ascii_width, plain_width) =
            (share(0.13, 215.0, 300.0), share(0.107, 120.0, 260.0), share(0.107, 200.0, 280.0), share(0.105, 110.0, 240.0));
        if self.strings {
            let mut open = true;
            let state = &mut self.strings_panel;
            dock(ui, true, "strings-panel", "String explorer", ascii_width.max(240.0), &mut open, |ui| crate::strings_explorer::panel(ui, state, doc, actions));
            self.strings = open;
        }
        if self.encoding_preview {
            let mut open = true;
            dock(ui, true, "encoding-panel", "Encoding Preview", plain_width, &mut open, |ui| encoding_panel(ui, doc));
            self.encoding_preview = open;
        }
        if self.clipboard_history {
            self.poll_clipboard(ui);
            let mut open = true;
            let items = &mut self.clip_items;
            dock(ui, true, "clipboard-panel", "Clipboard History", plain_width, &mut open, |ui| clipboard_panel(ui, items, actions));
            self.clipboard_history = open;
        }
        if self.ascii_table && self.control_sequences {
            let mut open = true;
            let table = &mut self.table;
            dock(ui, true, "sequences-panel", "Control sequences", plain_width.max(170.0), &mut open, |ui| char_table::sequences_panel(ui, table, doc, actions));
            self.control_sequences = open;
        }
        if self.ascii_table {
            let mut open = true;
            let table = &mut self.table;
            let sequences = &mut self.control_sequences;
            dock(ui, true, "ascii-panel", "Character table", ascii_width, &mut open, |ui| char_table::panel(ui, table, doc, sequences, actions));
            self.ascii_table = open;
        }
        if self.xml_manager {
            let mut open = true;
            let tree = &mut self.xml;
            dock(ui, true, "xml-panel", "XML manager", tree_width, &mut open, |ui| {
                tree_panel(ui, tree, doc, tabs, active, parse_xml, &['<'], "Attribute", actions);
            });
            self.xml_manager = open;
        }
        if self.json_manager {
            let mut open = true;
            let tree = &mut self.json;
            dock(ui, true, "json-panel", "JSON manager", tree_width, &mut open, |ui| {
                tree_panel(ui, tree, doc, tabs, active, parse_json, &['{', '['], "Key", actions);
            });
            self.json_manager = open;
        }
        if self.file_properties {
            let mut open = true;
            let properties = &mut self.properties;
            dock(ui, true, "properties-panel", "File properties", props_width, &mut open, |ui| properties_panel(ui, properties, doc, actions));
            self.file_properties = open;
        }

        if self.file_explorer {
            let mut open = true;
            let explorer = &mut self.explorer;
            dock(ui, false, "explorer-panel", "File explorer", 220.0, &mut open, |ui| explorer_panel(ui, explorer, doc, actions));
            self.file_explorer = open;
        }
        if self.open_files {
            let mut open = true;
            dock(ui, false, "open-files-panel", "Open files", 180.0, &mut open, |ui| open_files_panel(ui, tabs, active, actions));
            self.open_files = open;
        }
        if self.file_lists {
            let mut open = true;
            let recent = &mut self.recent;
            dock(ui, false, "file-lists-panel", "File lists", 220.0, &mut open, |ui| file_lists_panel(ui, recent, actions));
            self.file_lists = open;
        }
        if self.bookmarks {
            let mut open = true;
            dock(ui, false, "bookmarks-panel", "Bookmarks", 220.0, &mut open, |ui| bookmarks_panel(ui, doc, actions));
            self.bookmarks = open;
        }
        if self.functions {
            let mut open = true;
            let panel = &mut self.functions_panel;
            dock(ui, false, "functions-panel", "Functions", 220.0, &mut open, |ui| functions_panel(ui, panel, doc, actions));
            self.functions = open;
        }
        if self.tags {
            let mut open = true;
            let (panel, custom) = (&mut self.tags_panel, &mut self.custom_tags);
            dock(ui, false, "tags-panel", "Tags", 200.0, &mut open, |ui| tags_panel(ui, panel, custom, actions));
            self.tags = open;
        }
    }

    fn poll_clipboard(&mut self, ui: &Ui) {
        let now = ui.input(|i| i.time);
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(CLIPBOARD_POLL_SECONDS));
        if now - self.clip_last_poll < CLIPBOARD_POLL_SECONDS && self.clip_last_poll != 0.0 {
            return;
        }
        self.clip_last_poll = now.max(0.001);
        if let Ok(text) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
            remember_clip(&mut self.clip_items, text);
        }
    }
}

/// Adds to the history, newest first, without repeats. Nothing here is ever saved to disk.
fn remember_clip(items: &mut Vec<String>, text: String) {
    if text.is_empty() || text.len() > MAX_CLIPBOARD_ITEM_BYTES || items.first() == Some(&text) {
        return;
    }
    items.retain(|item| *item != text);
    items.insert(0, text);
    items.truncate(MAX_CLIPBOARD_ITEMS);
}

/// A docked panel with a title row and a close button.
fn dock(ui: &mut Ui, right: bool, id: &'static str, title: &str, width: f32, open: &mut bool, content: impl FnOnce(&mut Ui)) {
    let panel = if right { Panel::right(id) } else { Panel::left(id) };
    panel
        .default_size(width)
        .size_range(90.0..=900.0)
        .resizable(true)
        .frame(egui::Frame::new().fill(theme::window()).inner_margin(egui::Margin::symmetric(3, 2)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).color(theme::text()).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("\u{D7}").color(theme::text_dim())).frame(false)).on_hover_text("Close").clicked() {
                        *open = false;
                    }
                });
            });
            ui.add_space(2.0);
            content(ui);
        });
}

fn heading_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::text_dim()).size(12.5));
}

fn readable_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let v = bytes as f64;
    let short = if v >= KB * KB * KB {
        format!("{:.2} GB", v / (KB * KB * KB))
    } else if v >= KB * KB {
        format!("{:.1} MB", v / (KB * KB))
    } else if v >= KB {
        format!("{:.1} KB", v / KB)
    } else {
        return format!("{bytes} bytes");
    };
    format!("{short} ({} bytes)", grouped(bytes))
}

fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// ---------- File properties ----------

#[derive(Default, Clone)]
struct Stats {
    counts: Counts,
    current_size: Option<u64>,
}

#[derive(Default)]
struct Properties {
    key: (u64, u64),
    stats: Option<Stats>,
    last_update: f64,
    checksum_path: Option<PathBuf>,
    checksum: Option<Arc<Mutex<Option<String>>>>,
    /// An image's picture on the graphics card, for the place it came from; None if it couldn't be decoded.
    picture: Option<(String, Option<egui::TextureHandle>)>,
}

fn compute_stats(doc: &DocView<'_>) -> Stats {
    if doc.hex_mode {
        return Stats { current_size: Some(doc.hex.len() as u64), ..Stats::default() };
    }
    if doc.text.is_disk_backed() {
        let counts = Counts { lines: doc.text.line_count() as u64, ..Counts::default() };
        return Stats { counts, current_size: None };
    }
    let lines = doc.text.all_lines();
    Stats {
        counts: count_lines(&lines, doc.text.insert_eol().char_count()),
        current_size: doc.text.encode(doc.format, codec::Lossy::Refuse).ok().map(|b| b.len() as u64),
    }
}

pub fn format_time(time: std::io::Result<std::time::SystemTime>) -> String {
    match time {
        Ok(time) => chrono::DateTime::<chrono::Local>::from(time).format("%d/%m/%Y %H:%M:%S").to_string(),
        Err(_) => "Not available".into(),
    }
}

#[cfg(not(windows))]
fn attribute_names(meta: &std::fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode();
    let flags: String = [(0o400, 'r'), (0o200, 'w'), (0o100, 'x'), (0o040, 'r'), (0o020, 'w'), (0o010, 'x'), (0o004, 'r'), (0o002, 'w'), (0o001, 'x')].iter().map(|(bit, c)| if mode & bit != 0 { *c } else { '-' }).collect();
    format!("{flags} ({:o})", mode & 0o7777)
}

#[cfg(windows)]
fn attribute_names(meta: &std::fs::Metadata) -> String {
    use std::os::windows::fs::MetadataExt;
    let bits = meta.file_attributes();
    let names: Vec<&str> = [(0x1, "Read-only"), (0x2, "Hidden"), (0x4, "System"), (0x20, "Archive"), (0x800, "Compressed"), (0x4000, "Encrypted")]
        .iter()
        .filter(|(bit, _)| bits & bit != 0)
        .map(|(_, name)| *name)
        .collect();
    if names.is_empty() { "Normal".into() } else { names.join(", ") }
}

fn line_terminator_name(doc: &DocView<'_>) -> String {
    let counts = doc.text.eol_counts();
    if counts.is_mixed() {
        return format!("Mixed: {}", counts.describe());
    }
    match doc.text.insert_eol() {
        Eol::CrLf => "DOS (CR/LF)".to_string(),
        Eol::Lf => "Unix (LF)".to_string(),
        Eol::Cr => "Mac (CR)".to_string(),
        Eol::Atascii => "ATASCII EOL (0x9B)".to_string(),
        Eol::Nl => "EBCDIC (NL, 0x15)".to_string(),
        Eol::None => doc.format.newline.name(),
    }
}

fn bom_text(format: &TextFormat) -> String {
    match format.charset.bom().filter(|_| format.has_bom) {
        Some(mark) => hex_text(mark.iter().copied()),
        None => "None".to_string(),
    }
}

fn properties_panel(ui: &mut Ui, p: &mut Properties, doc: &DocView<'_>, actions: &mut Vec<Action>) {
    if doc.no_file {
        ui.label(RichText::new("No file open").color(theme::text_dim()));
        return;
    }
    let key = (doc.id, doc.version());
    let now = ui.input(|i| i.time);
    if p.stats.is_none() || p.key != key {
        if p.stats.is_none() || p.key.0 != doc.id || now - p.last_update >= REFRESH_SECONDS {
            p.stats = Some(compute_stats(doc));
            p.key = key;
            p.last_update = now;
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(REFRESH_SECONDS));
        }
    }
    let stats = p.stats.clone().unwrap_or_default();
    let meta = doc.path.and_then(|path| std::fs::metadata(path).ok());
    let mut rows_for_copy: Vec<(String, String)> = Vec::new();

    let height = ui.available_height() - 26.0;
    egui::ScrollArea::both().id_salt("properties-scroll").auto_shrink([false, false]).max_height(height.max(40.0)).show(ui, |ui| {
        section(ui, "Information", "properties-info", |ui| {
            let mut row = |ui: &mut Ui, label: &str, value: String| property_row(ui, label, &value, &mut rows_for_copy);
            row(ui, "Path:", doc.path.map_or_else(|| "Not saved yet".to_string(), |path| path.display().to_string()));
            row(ui, "Size (saved):", meta.as_ref().map_or_else(|| "Not saved yet".to_string(), |m| readable_size(m.len())));
            if doc.image.is_none() {
                row(ui, "Size (current):", stats.current_size.map_or_else(|| "Not counted in a file this large".to_string(), readable_size));
            }
            if let Some(m) = &meta {
                row(ui, "Date created:", format_time(m.created()));
                row(ui, "Date modified:", format_time(m.modified()));
                row(ui, "Date accessed:", format_time(m.accessed()));
                row(ui, "Attributes:", attribute_names(m));
            }
        });
        if let Some(image) = &doc.image {
            image_sections(ui, &mut p.picture, image, &mut rows_for_copy);
        }
        if doc.image.is_none() {
        section(ui, "Contents", "properties-contents", |ui| {
            let mut row = |ui: &mut Ui, label: &str, value: String| property_row(ui, label, &value, &mut rows_for_copy);
            if doc.hex_mode {
                row(ui, "Content type:", "Binary".into());
            } else {
                row(ui, "Encoding:", doc.format.encoding_name());
                if matches!(doc.format.certainty, codec::Certainty::Suggested | codec::Certainty::Remembered) {
                    row(ui, "Encoding is:", doc.format.certainty.describe().into());
                }
                row(ui, "BOM (if applicable):", bom_text(doc.format));
                row(ui, "Content type:", "Text".into());
                row(ui, "Line terminators:", line_terminator_name(doc));
                if doc.format.display != codec::Display::Plain {
                    row(ui, "Display profile:", doc.format.display.name().into());
                }
                row(ui, "Syntax highlighting:", doc.language.clone());
            }
        });
        }
        checksum_row(ui, p, doc, &mut rows_for_copy);
        if !doc.hex_mode && doc.image.is_none() {
            section(ui, "Statistics", "properties-stats", |ui| {
                if doc.text.is_disk_backed() {
                    property_row(ui, "Lines:", &grouped(stats.counts.lines), &mut rows_for_copy);
                    ui.label(RichText::new("The other counts would need the whole file to be read.").color(theme::text_faint()).size(12.0));
                    return;
                }
                let counts = &stats.counts;
                property_row(ui, "Characters:", &grouped(counts.characters), &mut rows_for_copy);
                property_row(ui, "Lines:", &grouped(counts.lines), &mut rows_for_copy);
                property_row(ui, "Non-empty lines:", &grouped(counts.non_empty), &mut rows_for_copy);
                property_row(ui, "Average line length:", &grouped(counts.average), &mut rows_for_copy);
                property_cells(ui, "Longest line:", |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let number = counts.longest_line;
                        if ui.link(RichText::new(grouped(number)).size(12.5)).on_hover_text("Go to this line").clicked() {
                            actions.push(Action::GoToLine(number.saturating_sub(1) as usize));
                        }
                        ui.label(RichText::new(format!("({} chars)", grouped(counts.longest_chars))).size(12.5));
                    });
                });
                rows_for_copy.push(("Longest line:".into(), format!("{} ({} chars)", counts.longest_line, counts.longest_chars)));
                property_row(ui, "Changed lines:", &grouped(doc.text.changed_line_count() as u64), &mut rows_for_copy);
            });
        }
    });
    ui.add_space(2.0);
    if ui.link(RichText::new("Copy all").size(12.5)).clicked() {
        let text: Vec<String> = rows_for_copy.iter().map(|(label, value)| format!("{label} {value}")).collect();
        actions.push(Action::CopyText(text.join("\r\n")));
    }
}

/// A disk image or archive's own sections, its picture, and the file chosen in it.
fn image_sections(ui: &mut Ui, picture: &mut Option<(String, Option<egui::TextureHandle>)>, image: &ImageView<'_>, rows: &mut Vec<(String, String)>) {
    if let Some(png) = image.picture {
        if picture.as_ref().is_none_or(|(place, _)| *place != image.place) {
            let texture = decode_png(png).map(|picture| ui.ctx().load_texture("image-picture", picture, egui::TextureOptions::NEAREST));
            *picture = Some((image.place.clone(), texture));
        }
        if let Some((_, Some(texture))) = picture {
            // Small pictures, as packages carry, are shown twice the size, in whole pixels.
            let size = texture.size_vec2();
            let scale = ((ui.available_width() - 8.0) / size.x).clamp(0.25, 2.0);
            let scale = if scale >= 1.0 { scale.floor() } else { scale };
            ui.add(egui::Image::new(&*texture).fit_to_exact_size(size * scale));
        }
    }
    for (index, info) in image.sections.iter().enumerate() {
        section(ui, &info.title, &format!("properties-image-{index}-{}", info.title), |ui| {
            for (label, value) in &info.rows {
                if label.is_empty() {
                    ui.add(egui::Label::new(RichText::new(value).size(12.5)).wrap());
                    rows.push((format!("{}:", info.title), value.clone()));
                } else {
                    property_row(ui, &format!("{label}:"), value, rows);
                }
            }
        });
    }
    if let Some(entry) = image.selected {
        section(ui, "Chosen in it", "properties-image-entry", |ui| {
            property_row(ui, "Name:", &entry.name, rows);
            if !entry.kind.is_empty() {
                property_row(ui, "Type:", &entry.kind, rows);
            }
            if !entry.is_dir {
                property_row(ui, "Size:", &readable_size(entry.size), rows);
            }
            if !entry.detail.is_empty() {
                property_row(ui, "Details:", &entry.detail, rows);
            }
            property_row(ui, "In:", &image.place, rows);
        });
    }
}

/// A PNG as egui takes it, whatever kind of PNG it is: palette, grey, with or without transparency.
fn decode_png(png: &[u8]) -> Option<egui::ColorImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut data = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut data).ok()?;
    data.truncate(frame.buffer_size());
    let size = [frame.width as usize, frame.height as usize];
    let rgba: Vec<u8> = match frame.color_type {
        png::ColorType::Rgba => data,
        png::ColorType::Rgb => data.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => data.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    (rgba.len() == size[0] * size[1] * 4).then(|| egui::ColorImage::from_rgba_unmultiplied(size, &rgba))
}

fn section(ui: &mut Ui, title: &str, id: &str, content: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).strong().size(12.5)).id_salt(id).default_open(true).show(ui, content);
}

/// Width of the labels in File properties, so the values line up in a column.
const LABEL_WIDTH: f32 = 84.0;

/// A label and its value side by side; the value wraps inside its own column.
fn property_cells(ui: &mut Ui, label: &str, value: impl FnOnce(&mut Ui)) {
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(LABEL_WIDTH, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(LABEL_WIDTH);
            ui.add(egui::Label::new(RichText::new(label).color(theme::text_dim()).size(12.5)).wrap());
        });
        // At least this wide, so a narrow panel scrolls rather than stacking one letter per line.
        let width = ui.available_width().max(80.0);
        ui.allocate_ui_with_layout(vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(width);
            value(ui);
        });
    });
}

fn property_row(ui: &mut Ui, label: &str, value: &str, rows: &mut Vec<(String, String)>) {
    property_cells(ui, label, |ui| {
        ui.add(egui::Label::new(RichText::new(value).size(12.5)).wrap());
    });
    rows.push((label.to_string(), value.to_string()));
}

/// The SHA-256 of the saved file, worked out in the background when asked for.
fn checksum_row(ui: &mut Ui, p: &mut Properties, doc: &DocView<'_>, rows: &mut Vec<(String, String)>) {
    let Some(path) = doc.path else {
        return;
    };
    if p.checksum_path.as_deref() != Some(path) {
        p.checksum_path = Some(path.to_path_buf());
        p.checksum = None;
    }
    property_cells(ui, "SHA-256:", |ui| {
        match &p.checksum {
            None => {
                if ui.link(RichText::new("Calculate...").size(12.5)).on_hover_text("Reads the saved file, however large").clicked() {
                    let slot = Arc::new(Mutex::new(None));
                    let (worker, target) = (slot.clone(), path.to_path_buf());
                    let ctx = ui.ctx().clone();
                    std::thread::spawn(move || {
                        let attempt = std::panic::catch_unwind(|| sha256_of_file(&target));
                        let result = match attempt {
                            Ok(Ok(value)) => value,
                            Ok(Err(e)) => format!("Couldn't read the file: {e}"),
                            Err(_) => "Couldn't work it out.".to_string(),
                        };
                        *worker.lock().unwrap() = Some(result);
                        ctx.request_repaint();
                    });
                    p.checksum = Some(slot);
                }
            }
            Some(slot) => match slot.lock().unwrap().as_ref() {
                Some(value) => {
                    ui.label(RichText::new(value).monospace().size(11.0));
                    rows.push(("SHA-256:".into(), value.clone()));
                }
                None => {
                    ui.spinner();
                }
            },
        }
    });
}

fn sha256_of_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let got = file.read(&mut buffer)?;
        if got == 0 {
            break;
        }
        hasher.update(&buffer[..got]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

// ---------- JSON and XML managers ----------

/// One row of the tree, in document order; `end` is where its children stop.
#[derive(Debug)]
struct Node {
    depth: usize,
    label: String,
    end: usize,
    expanded: bool,
    rows: Vec<(String, String)>,
}

#[derive(Default)]
struct TreeManager {
    key: (u64, u64),
    parsed_once: bool,
    last_update: f64,
    nodes: Vec<Node>,
    selected: usize,
    problem: Option<String>,
}

fn short_text(text: &str) -> String {
    const LIMIT: usize = 120;
    if text.chars().count() <= LIMIT {
        return text.to_string();
    }
    let mut out: String = text.chars().take(LIMIT).collect();
    out.push_str("...");
    out
}

fn json_short(value: &serde_json::Value) -> String {
    use serde_json::Value;
    match value {
        Value::Object(map) => format!("{{{}}}", map.len()),
        Value::Array(items) => format!("[{}]", items.len()),
        Value::String(s) => short_text(s),
        other => other.to_string(),
    }
}

fn add_json(nodes: &mut Vec<Node>, key: &str, value: &serde_json::Value, depth: usize) {
    use serde_json::Value;
    let index = nodes.len();
    let rows: Vec<(String, String)> = match value {
        Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), json_short(v))).collect(),
        Value::Array(items) => items.iter().enumerate().map(|(i, v)| (i.to_string(), json_short(v))).collect(),
        other => vec![(key.to_string(), json_short(other))],
    };
    let label = if key.is_empty() { json_short(value) } else { format!("{key}: {}", json_short(value)) };
    nodes.push(Node { depth, label, end: index + 1, expanded: depth < 2, rows });
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                add_json(nodes, k, v, depth + 1);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                add_json(nodes, &i.to_string(), v, depth + 1);
            }
        }
        _ => {}
    }
    nodes[index].end = nodes.len();
}

fn parse_json(text: &str) -> Result<Vec<Node>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| crate::tools::json_error_text(&e))?;
    let mut nodes = Vec::new();
    add_json(&mut nodes, "", &value, 0);
    Ok(nodes)
}

fn add_xml(nodes: &mut Vec<Node>, node: roxmltree::Node<'_, '_>, depth: usize) {
    let index = nodes.len();
    let rows: Vec<(String, String)> = node.attributes().map(|a| (a.name().to_string(), short_text(a.value()))).collect();
    let text = node.children().filter(|c| c.is_text()).filter_map(|c| c.text()).map(str::trim).collect::<Vec<_>>().join(" ");
    let name = node.tag_name().name();
    let label = if text.is_empty() { name.to_string() } else { format!("{name}: {}", short_text(&text)) };
    nodes.push(Node { depth, label, end: index + 1, expanded: depth < 2, rows });
    for child in node.children().filter(roxmltree::Node::is_element) {
        add_xml(nodes, child, depth + 1);
    }
    nodes[index].end = nodes.len();
}

fn parse_xml(text: &str) -> Result<Vec<Node>, String> {
    let options = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let document = roxmltree::Document::parse_with_options(text, options).map_err(|e| format!("This isn't valid XML: {e}"))?;
    let mut nodes = Vec::new();
    add_xml(&mut nodes, document.root_element(), 0);
    Ok(nodes)
}

#[allow(clippy::too_many_arguments)]
fn tree_panel(
    ui: &mut Ui,
    tree: &mut TreeManager,
    doc: &DocView<'_>,
    tabs: &[TabInfo],
    active: usize,
    parse: fn(&str) -> Result<Vec<Node>, String>,
    starts_with: &[char],
    first_column: &str,
    actions: &mut Vec<Action>,
) {
    let key = (doc.id, doc.version());
    let now = ui.input(|i| i.time);
    if tree.key != key || !tree.parsed_once {
        if !tree.parsed_once || tree.key.0 != doc.id || now - tree.last_update >= REFRESH_SECONDS {
            tree.key = key;
            tree.parsed_once = true;
            tree.last_update = now;
            tree.selected = 0;
            tree.nodes.clear();
            tree.problem = None;
            match doc.whole_text() {
                // A file that isn't this kind at all just leaves the panel empty, as in the original.
                Ok(text) if !text.trim_start().starts_with(starts_with) => {}
                Ok(text) => match parse(&text) {
                    Ok(nodes) => tree.nodes = nodes,
                    Err(problem) => tree.problem = Some(problem),
                },
                Err(problem) => tree.problem = Some(problem),
            }
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(REFRESH_SECONDS));
        }
    }

    ui.horizontal(|ui| {
        // Leaves more than the button needs: a row wider than its panel makes the panel grow every frame.
        let width = (ui.available_width() - 48.0).max(30.0);
        let shown = if tree.nodes.is_empty() && tree.problem.is_none() { String::new() } else { short_text(&doc.name) };
        egui::ComboBox::from_id_salt(first_column).width(width).selected_text(shown).show_ui(ui, |ui| {
            for (i, tab) in tabs.iter().enumerate() {
                if ui.selectable_label(i == active, &tab.title).clicked() {
                    actions.push(Action::SwitchTab(i));
                }
            }
        });
        if ui.button("...").on_hover_text("Open a file").clicked() {
            actions.push(Action::OpenDialog);
        }
    });

    let table_height = (ui.available_height() * 0.3).clamp(60.0, 260.0);
    let tree_height = (ui.available_height() - table_height - 8.0).max(30.0);
    let (tree_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), tree_height), Sense::hover());
    ui.painter().rect_stroke(tree_rect, 0.0, Stroke::new(1.0, theme::strong_line()), egui::StrokeKind::Inside);
    let mut tree_ui = ui.new_child(egui::UiBuilder::new().max_rect(tree_rect.shrink(2.0)).id_salt(("tree-ui", first_column)));
    if let Some(problem) = &tree.problem {
        tree_ui.label(RichText::new(problem).color(theme::text_dim()).size(12.0));
    } else {
        draw_tree(&mut tree_ui, tree, first_column);
    }
    ui.add_space(4.0);
    let rows: &[(String, String)] = tree.nodes.get(tree.selected).map_or(&[], |n| n.rows.as_slice());
    key_value_table(ui, first_column, rows, table_height);
}

fn draw_tree(ui: &mut Ui, tree: &mut TreeManager, salt: &str) {
    let mut visible = Vec::new();
    let mut i = 0;
    while i < tree.nodes.len() {
        visible.push(i);
        i = if tree.nodes[i].expanded { i + 1 } else { tree.nodes[i].end };
    }
    let row = ui.text_style_height(&egui::TextStyle::Body) + 2.0;
    egui::ScrollArea::both().id_salt(("tree", salt)).auto_shrink([false, false]).show_rows(ui, row, visible.len(), |ui, range| {
        for &index in &visible[range] {
            let (depth, has_children, expanded, label) = {
                let n = &tree.nodes[index];
                (n.depth, n.end > index + 1, n.expanded, n.label.clone())
            };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.add_space(depth as f32 * 12.0);
                if has_children {
                    let arrow = if expanded { "\u{25BC}" } else { "\u{25B6}" };
                    if ui.add(egui::Button::new(RichText::new(arrow).size(9.0)).frame(false)).clicked() {
                        tree.nodes[index].expanded = !expanded;
                    }
                } else {
                    ui.add_space(14.0);
                }
                let text = RichText::new(label).size(12.5);
                if ui.add(egui::Button::selectable(tree.selected == index, text).frame_when_inactive(false).wrap_mode(egui::TextWrapMode::Extend)).clicked() {
                    tree.selected = index;
                }
            });
        }
    });
}

/// The two-column table under a tree, ruled like the original even when it's empty.
fn key_value_table(ui: &mut Ui, first_column: &str, rows: &[(String, String)], height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    let line = Stroke::new(1.0, theme::strong_line());
    let row_h = 18.0;
    let middle = rect.min.x + rect.width() * 0.5;
    let font = FontId::proportional(12.5);
    painter.text(pos2(rect.min.x + 4.0, rect.min.y + 3.0), Align2::LEFT_TOP, first_column, font.clone(), theme::text_dim());
    painter.text(pos2(middle + 4.0, rect.min.y + 3.0), Align2::LEFT_TOP, "Value", font.clone(), theme::text_dim());
    painter.vline(middle, rect.y_range(), line);
    let first_y = rect.min.y + 20.0;
    let shown = ((rect.height() - 20.0) / row_h).floor() as usize;
    painter.hline(rect.x_range(), first_y, line);
    for i in 0..shown {
        let y = first_y + i as f32 * row_h;
        painter.hline(rect.x_range(), y + row_h, line);
        if let Some((name, value)) = rows.get(i) {
            let left = Rect::from_min_max(pos2(rect.min.x, y), pos2(middle - 2.0, y + row_h));
            let right = Rect::from_min_max(pos2(middle + 2.0, y), pos2(rect.max.x, y + row_h));
            painter.with_clip_rect(left).text(pos2(rect.min.x + 4.0, y + 2.0), Align2::LEFT_TOP, name, font.clone(), theme::text());
            painter.with_clip_rect(right).text(pos2(middle + 4.0, y + 2.0), Align2::LEFT_TOP, value, font.clone(), theme::text());
        }
    }
}

// ---------- Clipboard history ----------

fn clipboard_panel(ui: &mut Ui, items: &mut Vec<String>, actions: &mut Vec<Action>) {
    if items.is_empty() {
        ui.label(RichText::new("Clipboard history is empty").size(12.5));
        return;
    }
    let mut clear = false;
    ui.horizontal(|ui| clear = ui.small_button("Clear").clicked());
    egui::ScrollArea::vertical().id_salt("clipboard-scroll").auto_shrink([false, false]).show(ui, |ui| {
        for item in items.iter() {
            let first = item.lines().next().unwrap_or("");
            let more = item.lines().count().saturating_sub(1);
            let label = if more > 0 { format!("{} (+{more} lines)", short_text(first)) } else { short_text(first) };
            let button = egui::Button::new(RichText::new(label).size(12.5)).wrap_mode(egui::TextWrapMode::Truncate).min_size(vec2(ui.available_width(), 0.0));
            if ui.add(button).on_hover_text("Paste this").clicked() {
                actions.push(Action::Paste(item.clone()));
            }
        }
    });
    if clear {
        items.clear();
    }
}

// ---------- Functions ----------

/// How long typing has to pause before the list is worked out again.
const FUNCTIONS_PAUSE_SECONDS: f64 = 0.6;

/// Assembler source's outline: where each ORG puts the code, the labels with their values (local labels under
/// the label they belong to), and the macros. None for anything that isn't assembler source.
pub fn assembler_outline(intel: &crate::code_intel::CodeIntel) -> Option<Vec<(usize, String, String)>> {
    let assembly = intel.assembly.as_ref().filter(|_| intel.cpu.is_some())?;
    let mut items: Vec<(usize, String, String)> = Vec::new();
    // Each ORG: the address the first line after it was put at.
    let mut last_org = None;
    for placed in assembly.lines.iter().filter(|p| p.file == intel.file) {
        if last_org.is_none_or(|(address, len): (i64, usize)| placed.address != address + len as i64 && !placed.bytes.is_empty()) && !placed.bytes.is_empty() {
            items.push((placed.line, format!("at ${:04X}", placed.address), String::new()));
        }
        if !placed.bytes.is_empty() {
            last_org = Some((placed.address, placed.bytes.len()));
        }
    }
    for d in assembly.definitions.iter().filter(|d| d.file == intel.file) {
        let value = assembly.symbols.get(&d.name).map_or_else(String::new, |v| format!("${:04X}", v & 0xFFFF_FFFF));
        let name = if d.written == d.name { d.name.clone() } else { format!("    {}", d.written) };
        items.push((d.line, name, value));
    }
    for m in assembly.macros.iter().filter(|m| m.file == intel.file) {
        items.push((m.line, format!("macro {}", m.name), String::new()));
    }
    items.sort_by_key(|(line, name, _)| (*line, !name.starts_with("at $")));
    Some(items)
}

fn functions_panel(ui: &mut Ui, panel: &mut FunctionsPanel, doc: &DocView<'_>, actions: &mut Vec<Action>) {
    let say = |ui: &mut Ui, text: &str| {
        ui.label(RichText::new(text).color(theme::text_faint()).size(12.5));
    };
    if doc.no_file {
        return say(ui, "No file open");
    }
    if doc.hex_mode {
        return say(ui, "Switch to text mode to list functions.");
    }
    if doc.text.is_disk_backed() {
        return say(ui, "This file is too large to list. It is being kept on disk.");
    }
    // Assembler source: its outline, from what the assembler knows.
    if let Some(items) = doc.text.intel.as_deref().and_then(assembler_outline) {
        ui.add(egui::TextEdit::singleline(&mut panel.filter).hint_text("Filter").desired_width(f32::INFINITY));
        ui.add_space(2.0);
        if items.is_empty() {
            return say(ui, "No labels yet.");
        }
        let filter = panel.filter.to_lowercase();
        let shown: Vec<&(usize, String, String)> = items.iter().filter(|(_, name, _)| filter.is_empty() || name.to_lowercase().contains(&filter)).collect();
        let row = ui.text_style_height(&egui::TextStyle::Body) + 3.0;
        egui::ScrollArea::vertical().id_salt("functions-scroll").auto_shrink([false, false]).show_rows(ui, row, shown.len(), |ui, range| {
            for (line, name, value) in &shown[range] {
                let words = if value.is_empty() { name.clone() } else { format!("{name}   {value}") };
                let button = egui::Button::new(RichText::new(words).size(12.5).monospace()).frame_when_inactive(false).wrap_mode(egui::TextWrapMode::Truncate).min_size(vec2(ui.available_width(), 0.0));
                if ui.add(button).on_hover_text(format!("Line {}", line + 1)).clicked() {
                    actions.push(Action::GoToLine(*line));
                }
            }
        });
        return;
    }
    let (Some(engine), Some(syntax)) = (crate::highlight::engine(), doc.syntax) else {
        return say(ui, "This file has no language, so its functions can't be told apart. Choose one from Tools, Colour as.");
    };
    let key = (doc.id, doc.text.version());
    let now = ui.input(|i| i.time);
    let finished = panel.job.as_ref().and_then(|job| job.result.lock().unwrap().take().map(|found| (job.key, found)));
    if let Some((done, found)) = finished {
        panel.key = Some(done);
        panel.items = found;
        panel.job = None;
    }
    if panel.key != Some(key) {
        if panel.job.as_ref().is_some_and(|job| job.key != key) {
            panel.job.take().unwrap().stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        // Another file can show straight away; while typing, it waits for a pause.
        let waited = panel.key.is_none_or(|(id, _)| id != doc.id) || now - panel.started >= FUNCTIONS_PAUSE_SECONDS;
        if panel.job.is_none() && waited {
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let result = std::sync::Arc::new(std::sync::Mutex::new(None));
            let (lines, worker_stop, worker_result) = (doc.text.all_lines(), stop.clone(), result.clone());
            std::thread::spawn(move || {
                let found = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| engine.list_functions(syntax, &lines, &worker_stop))).unwrap_or_default();
                *worker_result.lock().unwrap() = Some(found);
            });
            panel.job = Some(FunctionJob { key, stop, result });
            panel.started = now;
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    ui.add(egui::TextEdit::singleline(&mut panel.filter).hint_text("Filter").desired_width(f32::INFINITY));
    ui.add_space(2.0);
    if panel.items.is_empty() {
        return say(ui, if panel.key == Some(key) { "No functions found." } else { "Looking..." });
    }
    let filter = panel.filter.to_lowercase();
    let shown: Vec<&(usize, String)> = panel.items.iter().filter(|(_, name)| filter.is_empty() || name.to_lowercase().contains(&filter)).collect();
    let row = ui.text_style_height(&egui::TextStyle::Body) + 3.0;
    egui::ScrollArea::vertical().id_salt("functions-scroll").auto_shrink([false, false]).show_rows(ui, row, shown.len(), |ui, range| {
        for (line, name) in &shown[range] {
            let button = egui::Button::new(RichText::new(name).size(12.5)).frame_when_inactive(false).wrap_mode(egui::TextWrapMode::Truncate).min_size(vec2(ui.available_width(), 0.0));
            if ui.add(button).on_hover_text(format!("Line {}", line + 1)).clicked() {
                actions.push(Action::GoToLine(*line));
            }
        }
    });
}

// ---------- Tags ----------

fn tags_panel(ui: &mut Ui, panel: &mut TagsPanel, custom: &mut Vec<Tag>, actions: &mut Vec<Action>) {
    if panel.group.is_empty() {
        panel.group = BUILT_IN_TAGS[0].0.to_string();
    }
    egui::ComboBox::from_id_salt("tag-group").selected_text(panel.group.as_str()).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
        for name in BUILT_IN_TAGS.iter().map(|(name, _)| *name).chain([MY_TAGS]) {
            ui.selectable_value(&mut panel.group, name.to_string(), name);
        }
    });
    ui.add_space(2.0);
    let mut gone = None;
    egui::ScrollArea::vertical().id_salt("tags-scroll").auto_shrink([false, false]).show(ui, |ui| {
        let tag_button = |ui: &mut Ui, name: &str, before: &str, after: &str, actions: &mut Vec<Action>| {
            let button = egui::Button::new(RichText::new(name).size(12.5)).frame_when_inactive(false).min_size(vec2(ui.available_width(), 0.0));
            if ui.add(button).on_hover_text(format!("{before}...{after}")).clicked() {
                actions.push(Action::WrapTag(before.to_string(), after.to_string()));
            }
        };
        if panel.group == MY_TAGS {
            for (place, tag) in custom.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.small_button("\u{D7}").on_hover_text("Take off the list").clicked() {
                        gone = Some(place);
                    }
                    tag_button(ui, &tag.name, &tag.before, &tag.after, actions);
                });
            }
            ui.add_space(6.0);
            ui.label(RichText::new("Add a tag").color(theme::text_dim()).size(12.5));
            ui.add(egui::TextEdit::singleline(&mut panel.name).hint_text("Name").desired_width(f32::INFINITY));
            ui.add(egui::TextEdit::singleline(&mut panel.before).hint_text("Text before").desired_width(f32::INFINITY));
            ui.add(egui::TextEdit::singleline(&mut panel.after).hint_text("Text after").desired_width(f32::INFINITY));
            let ready = !panel.name.trim().is_empty() && !(panel.before.is_empty() && panel.after.is_empty());
            if ui.add_enabled(ready, egui::Button::new("Add")).clicked() {
                custom.push(Tag { name: panel.name.trim().to_string(), before: std::mem::take(&mut panel.before), after: std::mem::take(&mut panel.after) });
                panel.name.clear();
            }
        } else if let Some((_, tags)) = BUILT_IN_TAGS.iter().find(|(name, _)| *name == panel.group) {
            for (name, before, after) in *tags {
                tag_button(ui, name, before, after, actions);
            }
        }
    });
    if let Some(place) = gone {
        custom.remove(place);
    }
}

// ---------- Encoding preview ----------

fn hex_text(bytes: impl IntoIterator<Item = u8>) -> String {
    bytes.into_iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

fn encoding_panel(ui: &mut Ui, doc: &DocView<'_>) {
    if doc.hex_mode {
        ui.label("Switch to text mode to preview encodings.");
        return;
    }
    let selection = doc.text.selection_for_preview();
    ui.label(RichText::new(if selection.is_empty() { "The character at the cursor" } else { "The selected text" }).color(theme::text_dim()).size(12.0));
    let text: String = if selection.is_empty() { doc.text.char_at_caret().map(String::from).unwrap_or_default() } else { selection };
    if text.is_empty() {
        ui.label(RichText::new("There's nothing here to preview.").color(theme::text_faint()));
        return;
    }
    let latin1 = if text.chars().all(|c| (c as u32) < 256) { hex_text(text.chars().map(|c| c as u32 as u8)) } else { "Can't be stored".to_string() };
    let own: Option<Vec<u8>> = text.chars().map(|c| codec::char_bytes(c, doc.format.charset, doc.format.state)).collect::<Option<Vec<_>>>().map(|parts| parts.concat());
    let own = own.map_or_else(|| "Can't be stored".to_string(), hex_text);
    let points = text.chars().map(|c| format!("U+{:04X}", c as u32)).collect::<Vec<_>>().join(" ");
    egui::ScrollArea::vertical().id_salt("encoding-scroll").auto_shrink([false, false]).show(ui, |ui| {
        for (name, value) in [
            ("Text", text.clone()),
            ("Code points", points),
            ("In this file", own),
            ("UTF-8", hex_text(text.bytes())),
            ("UTF-16 LE", hex_text(text.encode_utf16().flat_map(u16::to_le_bytes))),
            ("UTF-16 BE", hex_text(text.encode_utf16().flat_map(u16::to_be_bytes))),
            ("Latin-1", latin1),
        ] {
            heading_label(ui, name);
            ui.label(RichText::new(value).monospace().size(12.0));
            ui.add_space(4.0);
        }
    });
}

// ---------- File explorer, open files, bookmarks ----------

#[derive(Default)]
struct Explorer {
    folder: Option<PathBuf>,
    following: Option<PathBuf>,
    entries: Vec<(String, bool)>,
    loaded_for: Option<PathBuf>,
    problem: Option<String>,
}

fn explorer_panel(ui: &mut Ui, e: &mut Explorer, doc: &DocView<'_>, actions: &mut Vec<Action>) {
    let doc_folder = doc.path.and_then(Path::parent).map(Path::to_path_buf);
    if doc_folder != e.following {
        e.following.clone_from(&doc_folder);
        if doc_folder.is_some() {
            e.folder = doc_folder;
        }
    }
    if e.folder.is_none() {
        e.folder = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    }
    let Some(folder) = e.folder.clone() else {
        return;
    };
    if e.loaded_for.as_ref() != Some(&folder) {
        e.loaded_for = Some(folder.clone());
        e.entries.clear();
        e.problem = None;
        match std::fs::read_dir(&folder) {
            Ok(list) => {
                for entry in list.flatten() {
                    let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
                    e.entries.push((entry.file_name().to_string_lossy().into_owned(), is_dir));
                }
                e.entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
            }
            Err(problem) => e.problem = Some(problem.to_string()),
        }
    }
    ui.horizontal(|ui| {
        let up = folder.parent().map(Path::to_path_buf);
        if ui.add_enabled(up.is_some(), egui::Button::new("Up")).clicked()
            && let Some(up) = up
        {
            e.folder = Some(up);
        }
        if ui.button("Refresh").clicked() {
            e.loaded_for = None;
        }
    });
    ui.label(RichText::new(folder.display().to_string()).color(theme::text_dim()).size(11.5));
    if let Some(problem) = &e.problem {
        ui.label(RichText::new(problem).color(theme::text_faint()));
    }
    let row = ui.text_style_height(&egui::TextStyle::Body) + 3.0;
    let entries = std::mem::take(&mut e.entries);
    let mut enter = None;
    egui::ScrollArea::both().id_salt("explorer-scroll").auto_shrink([false, false]).show_rows(ui, row, entries.len(), |ui, range| {
        for (name, is_dir) in &entries[range] {
            let label = if *is_dir { format!("\u{25B8} {name}") } else { name.clone() };
            if ui.add(egui::Button::selectable(false, RichText::new(label).size(12.5)).frame_when_inactive(false).wrap_mode(egui::TextWrapMode::Extend)).clicked() {
                if *is_dir {
                    enter = Some(folder.join(name));
                } else {
                    actions.push(Action::OpenPath(folder.join(name)));
                }
            }
        }
    });
    e.entries = entries;
    if let Some(next) = enter {
        e.folder = Some(next);
    }
}

fn open_files_panel(ui: &mut Ui, tabs: &[TabInfo], active: usize, actions: &mut Vec<Action>) {
    egui::ScrollArea::both().id_salt("open-files-scroll").auto_shrink([false, false]).show(ui, |ui| {
        for (i, tab) in tabs.iter().enumerate() {
            let button = egui::Button::selectable(i == active, RichText::new(&tab.title).size(12.5)).frame_when_inactive(false).wrap_mode(egui::TextWrapMode::Extend);
            let mut response = ui.add(button);
            if let Some(path) = &tab.path {
                response = response.on_hover_text(path.display().to_string());
            }
            if response.clicked() {
                actions.push(Action::SwitchTab(i));
            }
        }
    });
}

/// How many recently opened files are remembered.
const MAX_RECENT: usize = 30;

/// Puts a file at the top of the list, without repeats. Windows paths don't care about case.
/// Takes files that are no longer there off the recent list.
pub fn clean_recent(list: &mut Vec<PathBuf>) {
    list.retain(|path| path.exists());
}

pub fn note_recent(list: &mut Vec<PathBuf>, path: &Path) {
    list.retain(|known| !known.as_os_str().eq_ignore_ascii_case(path.as_os_str()));
    list.insert(0, path.to_path_buf());
    list.truncate(MAX_RECENT);
}

fn file_lists_panel(ui: &mut Ui, recent: &mut Vec<PathBuf>, actions: &mut Vec<Action>) {
    ui.label(RichText::new("Recently opened").color(theme::text_dim()).size(12.0));
    if recent.is_empty() {
        ui.label(RichText::new("Files you open will be listed here.").color(theme::text_faint()).size(12.0));
        return;
    }
    let mut clear = false;
    let height = (ui.available_height() - 28.0).max(40.0);
    egui::ScrollArea::vertical().id_salt("file-lists-scroll").auto_shrink([false, false]).max_height(height).show(ui, |ui| {
        for path in recent.iter() {
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            let exists = path.exists();
            let color = if exists { theme::text() } else { theme::text_faint() };
            let button = egui::Button::new(RichText::new(name).size(12.5).color(color)).frame_when_inactive(false).wrap_mode(egui::TextWrapMode::Truncate).min_size(vec2(ui.available_width(), 0.0));
            let tip = if exists { path.display().to_string() } else { format!("{} (no longer there)", path.display()) };
            if ui.add(button).on_hover_text(tip).clicked() {
                actions.push(Action::OpenPath(path.clone()));
            }
        }
    });
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        if ui.button("Clean up").on_hover_text("Take files that are no longer there off the list").clicked() {
            clean_recent(recent);
        }
        clear = ui.button("Clear list").clicked() || clear;
    });
    if clear {
        recent.clear();
    }
}

fn bookmarks_panel(ui: &mut Ui, doc: &DocView<'_>, actions: &mut Vec<Action>) {
    if doc.hex_mode {
        ui.label("Bookmarks are for text mode.");
        return;
    }
    let marks = doc.text.bookmarks();
    if marks.is_empty() {
        ui.label("No bookmarks yet.");
        ui.label(RichText::new("Press Ctrl+F2 on a line, or click beside its number.").color(theme::text_faint()).size(12.0));
        return;
    }
    egui::ScrollArea::both().id_salt("bookmarks-scroll").auto_shrink([false, false]).show(ui, |ui| {
        for line in marks {
            let label = format!("{}: {}", line + 1, short_text(doc.text.line(line).trim()));
            if ui.add(egui::Button::selectable(false, RichText::new(label).size(12.5)).frame_when_inactive(false).wrap_mode(egui::TextWrapMode::Extend)).clicked() {
                actions.push(Action::GoToLine(line));
            }
        }
    });
}

/// A marker shared with the editor's gutter, so the two stay the same colour.
pub const BOOKMARK_COLOR: Color32 = Color32::from_rgb(208, 84, 62);

#[cfg(test)]
mod feature_tests {
    use super::*;

    #[test]
    fn custom_tags_survive_the_settings_file_with_their_line_breaks_and_equals_signs() {
        let mut panels = Panels::default();
        panels.custom_tags.push(Tag { name: "Box".into(), before: "<div class=\"a\">\n".into(), after: "\n</div>".into() });
        panels.functions = true;
        panels.tags = true;
        let mut again = Panels::default();
        again.read_settings(&panels.to_settings());
        assert_eq!(again.custom_tags, panels.custom_tags);
        assert!(again.functions && again.tags);
        again.hide_all();
        assert!(!again.functions && !again.tags);
    }

    #[test]
    fn a_broken_tag_line_is_ignored() {
        let mut panels = Panels::default();
        panels.read_settings("custom_tag=only a name\n");
        assert!(panels.custom_tags.is_empty());
    }

    #[test]
    fn built_in_tags_all_put_something_in() {
        for (_, tags) in BUILT_IN_TAGS {
            for (name, before, after) in tags {
                assert!(!name.is_empty() && !(before.is_empty() && after.is_empty()), "{name}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_open() -> Panels {
        Panels {
            file_properties: true,
            json_manager: true,
            xml_manager: true,
            ascii_table: true,
            clipboard_history: true,
            encoding_preview: true,
            file_explorer: true,
            open_files: true,
            bookmarks: true,
            ..Panels::default()
        }
    }

    /// Draws the panels for some frames and returns how wide the text area is left after each.
    fn text_width_over_frames(panels: &mut Panels, text: &TextBuffer, frames: usize, size: egui::Vec2) -> Vec<f32> {
        let ctx = egui::Context::default();
        let hex = HexBuffer::default();
        let format = TextFormat::default();
        let tabs = [TabInfo { title: "one.txt".into(), path: None }];
        let mut widths = Vec::new();
        for frame in 0..frames {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(frame as f64 / 60.0),
                predicted_dt: 1.0 / 60.0,
                ..Default::default()
            };
            let mut left = 0.0;
            let mut output = ctx.run_ui(raw, |ui| {
                let doc = DocView { id: 1, text, hex: &hex, format: &format, path: None, hex_mode: false, name: "one.txt".into(), language: "No highlighting".into(), no_file: false, syntax: None, image: None };
                let mut actions = Vec::new();
                panels.show(ui, &doc, &tabs, 0, &mut actions);
                left = ui.available_rect_before_wrap().width();
            });
            output.textures_delta.clear();
            widths.push(left);
        }
        widths
    }

    #[test]
    fn panels_keep_their_width_and_leave_the_text_room() {
        let mut text = TextBuffer::default();
        text.set_content(vec![r#"{"name": "a long value to fill the tree and the table", "list": [1, 2, 3]}"#.to_string()]);
        // The five panels in the person's picture, in a window of the app's starting size.
        let mut panels = Panels { file_properties: true, json_manager: true, xml_manager: true, ascii_table: true, clipboard_history: true, ..Panels::default() };
        let widths = text_width_over_frames(&mut panels, &text, 240, vec2(1280.0, 720.0));
        assert!((widths[5] - widths[239]).abs() < 1.0, "the panels crept wider: {} to {}", widths[5], widths[239]);
        assert!(widths[239] > 400.0, "the text is squeezed to {}", widths[239]);
        // Every panel at once, with text that gives each something to show.
        let mut every = all_open();
        let widths = text_width_over_frames(&mut every, &text, 240, vec2(1280.0, 720.0));
        assert!((widths[5] - widths[239]).abs() < 1.0, "with all open they crept wider: {} to {}", widths[5], widths[239]);
    }

    #[test]
    fn a_plain_text_file_leaves_the_managers_empty_without_a_complaint() {
        let mut text = TextBuffer::default();
        text.set_content(vec!["just some notes".to_string()]);
        let mut panels = Panels { json_manager: true, xml_manager: true, ..Panels::default() };
        text_width_over_frames(&mut panels, &text, 3, vec2(1280.0, 720.0));
        assert!(panels.json.nodes.is_empty() && panels.json.problem.is_none());
        assert!(panels.xml.nodes.is_empty() && panels.xml.problem.is_none());
        text.set_content(vec!["{broken".to_string()]);
        // Panels catch up with changes twice a second, so give it a second's worth of frames.
        text_width_over_frames(&mut panels, &text, 60, vec2(1280.0, 720.0));
        assert!(panels.json.problem.is_some(), "something that looks like JSON but isn't does get a message");
    }

    #[test]
    fn panel_settings_are_remembered() {
        let mut panels = Panels { json_manager: true, ascii_table: true, ..Panels::default() };
        let text = panels.to_settings();
        panels.hide_all();
        assert!(!panels.json_manager);
        panels.read_settings(&text);
        assert!(panels.json_manager && panels.ascii_table && !panels.xml_manager);
        panels.read_settings("xml_manager=maybe\nnonsense\nfile_properties=true");
        assert!(panels.file_properties && !panels.xml_manager);
    }

    #[test]
    fn json_becomes_a_tree_with_key_value_rows() {
        let nodes = parse_json(r#"{"name":"edit","list":[1,{"a":true}],"none":null}"#).unwrap();
        assert_eq!(nodes[0].label, "{3}");
        assert_eq!(nodes[0].rows[0], ("name".to_string(), "edit".to_string()));
        assert_eq!(nodes[0].end, nodes.len());
        let list = nodes.iter().position(|n| n.label == "list: [2]").unwrap();
        assert_eq!(nodes[list].rows[1], ("1".to_string(), "{1}".to_string()));
        assert_eq!(nodes[list].end, list + 4, "the list holds 1, the object, and a");
        assert!(parse_json("{not json").unwrap_err().starts_with("JSON error (1, "));
    }

    #[test]
    fn xml_becomes_a_tree_with_attribute_rows() {
        let nodes = parse_xml(r#"<?xml version="1.0"?><root a="1"><item id="x">hello</item><item/></root>"#).unwrap();
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].label, "root");
        assert_eq!(nodes[0].rows, [("a".to_string(), "1".to_string())]);
        assert_eq!(nodes[1].label, "item: hello");
        assert_eq!(nodes[1].depth, 1);
        assert!(parse_xml("<a><b></a>").unwrap_err().starts_with("This isn't valid XML"));
    }

    #[test]
    fn recent_files_are_newest_first_without_repeats_and_limited() {
        let mut list = Vec::new();
        note_recent(&mut list, Path::new(r"C:\a.txt"));
        note_recent(&mut list, Path::new(r"C:\b.txt"));
        note_recent(&mut list, Path::new(r"c:\A.TXT"));
        assert_eq!(list, [PathBuf::from(r"c:\A.TXT"), PathBuf::from(r"C:\b.txt")]);
        for i in 0..100 {
            note_recent(&mut list, &PathBuf::from(format!(r"C:\{i}.txt")));
        }
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list[0], PathBuf::from(r"C:\99.txt"));
    }

    #[test]
    fn clipboard_history_keeps_newest_first_without_repeats() {
        let mut items = Vec::new();
        remember_clip(&mut items, "one".into());
        remember_clip(&mut items, "two".into());
        remember_clip(&mut items, "two".into());
        remember_clip(&mut items, "one".into());
        remember_clip(&mut items, String::new());
        assert_eq!(items, ["one", "two"]);
        for i in 0..100 {
            remember_clip(&mut items, i.to_string());
        }
        assert_eq!(items.len(), MAX_CLIPBOARD_ITEMS);
        remember_clip(&mut items, "x".repeat(MAX_CLIPBOARD_ITEM_BYTES + 1));
        assert_eq!(items[0], "99", "very large items aren't kept");
    }

    #[test]
    fn sizes_and_checksums() {
        assert_eq!(readable_size(167_450), "163.5 KB (167,450 bytes)");
        assert_eq!(readable_size(12), "12 bytes");
        assert_eq!(grouped(1234567), "1,234,567");
        let path = std::env::temp_dir().join(format!("limoniter-sha-{}.txt", std::process::id()));
        std::fs::write(&path, "abc").unwrap();
        assert_eq!(sha256_of_file(&path).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        std::fs::remove_file(&path).unwrap();
    }
}
