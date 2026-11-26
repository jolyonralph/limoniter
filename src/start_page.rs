//! The start page, shown once every tab is closed, as VS Code's Welcome page is: the usual first steps, recent
//! work a click away, and the guide. Limoniter still starts with a new empty file; this takes the place of the
//! empty window that used to follow the last tab. Settings, Appearance, Show the start page turns it off.
//!
//! Every link is a button a screen reader names and Tab and Enter reach, and files dropped on it open as they do
//! anywhere on the window. The guide's pages and What's new are here too, opened as pages from Help as well.

use super::*;
use crate::ribbon_icons::{self, Icon};
use egui::{Rect, Sense, Stroke, pos2, vec2};

/// A page of the guide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Guide {
    GettingStarted,
    Folder,
    Assembling,
}

impl Guide {
    pub const ALL: [Self; 3] = [Self::GettingStarted, Self::Folder, Self::Assembling];

    pub fn title(self) -> &'static str {
        match self {
            Self::GettingStarted => "Getting started",
            Self::Folder => "Working on a folder of code",
            Self::Assembling => "Assembling and running",
        }
    }

    /// The page, a line at a time: "## " starts a heading, "- " a point, and anything else is a paragraph. Each is
    /// translated whole.
    fn lines(self) -> &'static [&'static str] {
        match self {
            Self::GettingStarted => &[
                "Limoniter is a text and hex editor. It opens almost any file: text in any encoding, pictures, disk and tape images, music, and the programs of older computers.",
                "## Opening and making files",
                "- File, New (Ctrl+N) makes an empty file. File, New as makes one of a kind, such as Spectrum BASIC or an Amiga demo, with something to start from.",
                "- File, Open (Ctrl+O) opens a file, and so does dropping one on the window. File, Open recent lists the last few.",
                "- A file that isn't text opens in hex, a byte at a time. View, Hex switches between the two.",
                "## Finding a command",
                "- F1 finds any command by what it does: type a word or two, such as sort or wrap, and press Enter.",
                "- The menus are arranged by what things are for. If you'd rather find things by sight, the ribbon switch at the top right, beside Find a command, puts tabs of buttons with pictures in their place.",
                "## Saving",
                "- File, Save (Ctrl+S) keeps the file's own encoding and line endings, as it was opened. Format, Encoding changes them.",
                "- If Limoniter ever stops unexpectedly, your unsaved edits are offered again when it next starts.",
                "## Going further",
                "- Help, Guide has two more pages: Working on a folder of code, and Assembling and running. Help, How things work covers everything in short.",
            ],
            Self::Folder => &[
                "Any folder can be worked on as a whole, as it is in VS Code: every file in it a few keys away, searched and changed together.",
                "## Opening the folder",
                "- File, Open folder makes the folder a project, with the Explorer down the side. Files are made, renamed, moved and deleted there, and Git's view of each is shown beside it.",
                "- The files you had open come back the next time the folder's opened.",
                "## Getting about",
                "- Go to file (Ctrl+E) finds a file from a few letters of its name. Go to symbol finds a function or label: Ctrl+T in the folder, Ctrl+Shift+O in this file.",
                "- The Search panel (Ctrl+Shift+F) searches every file as you type, and can replace in all of them.",
                "- F12 goes to a definition, Alt+F12 shows it without leaving your place, and Shift+F12 finds every use. Alt+Left goes back.",
                "## Building, running and testing",
                "- Tools, Tasks finds the folder's own commands for Cargo, Make, npm, Go and CMake. Ctrl+Shift+B builds and Ctrl+Shift+T tests.",
                "- The terminal (Ctrl+`) opens in the folder. F5 runs a command, and Ctrl+F5 debugs Python, C, C++ and Rust.",
                "## Git",
                "- Tools, Git, Source control (Ctrl+Shift+G) shows what's changed, ready to commit. New to Git? The Git wizard takes you through it a step at a time.",
            ],
            Self::Assembling => &[
                "Limoniter assembles code for the Z80, 6502, 6809, 68000, x86 and more, and runs it in an emulator or on the machine itself.",
                "## Starting",
                "- File, New as has starters to change: Spectrum BASIC, a C64 demo, an Amiga demo, a Vectrex cartridge and many more. Each assembles and runs just as it is.",
                "## Assembling",
                "- File, Run on, Assemble and save assembles the file in front, with Limoniter's own assembler or another chosen in Assemblers. Problems are marked on their lines.",
                "- Tools, General, Disassemble turns a program back into source.",
                "## Running",
                "- F9 (File, Run on machine) runs the file in front: in the built-in Spectrum, or in an emulator that's installed, such as FS-UAE, Hatari, VICE, Fuse or MAME.",
                "- File, Send to device sends it to a real machine: a memory card, a Spectrum Next, an Ultimate 64, or a folder.",
                "## Pictures and sound",
                "- Tools has editors for characters and sprites, tile maps and sound effects, and converters for pictures, each for the machine you're writing for.",
                "- Help, How To Code is a whole guide to writing Amiga demos, with examples that run.",
            ],
        }
    }

    /// The name of the page's tab.
    fn tab_name(self) -> String {
        format!("{}.md", crate::i18n::tr(self.title()))
    }
}

/// A guide page, as Markdown, in the language chosen.
pub(super) fn guide_markdown(page: Guide) -> String {
    let mut out = format!("# {}\n", crate::i18n::tr(page.title()));
    for line in page.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            out += &format!("\n## {}\n\n", crate::i18n::tr(heading));
        } else if let Some(point) = line.strip_prefix("- ") {
            out += &format!("- {}\n", crate::i18n::tr(point));
        } else {
            out += &format!("\n{}\n", crate::i18n::tr(line));
        }
    }
    out
}

const WHATS_NEW: &str = include_str!("../WHATS-NEW.md");

/// What's new in a version, from WHATS-NEW.md: its own notes if it has them, otherwise the newest there are, with
/// the version they're for.
pub(super) fn whats_new(version: &str) -> (String, String) {
    let text = WHATS_NEW.replace("\r\n", "\n");
    let sections: Vec<(&str, &str)> = text.split("\n## ").skip(1).filter_map(|section| section.split_once('\n')).collect();
    let (shown, body) = sections.iter().find(|(heading, _)| heading.trim() == version).or(sections.first()).copied().unwrap_or((version, ""));
    let shown = shown.trim().to_string();
    (shown.clone(), format!("# {}\n{}", crate::i18n::tf!("What's new in Limoniter {}", shown), body.trim_end()))
}

/// Short tips, each with the command whose keys go with it, shown one at a time on the start page.
const TIPS: [(&str, Command); 12] = [
    ("Find any command by what it does, without hunting through the menus.", Command::CommandPalette),
    ("Go to any file in the folder from a few letters of its name.", Command::GoToFile),
    ("Search every file in the folder as you type.", Command::SearchPanel),
    ("See any file as bytes, and back again.", Command::ViewHex),
    ("Run the program in front on its machine, in an emulator or the real thing.", Command::RunOnMachine),
    ("Snippets put in the text you type again and again.", Command::Snippets),
    ("Copy the line you're on, or the selection, just below it.", Command::Duplicate),
    ("Bookmark a line to come back to it; F2 goes to the next.", Command::ToggleBookmark),
    ("Narrow to the selection to work on part of a file as if it were all of it.", Command::Narrow),
    ("Open another file beside the one in front.", Command::OpenBeside),
    ("A terminal in the window, in the file's folder.", Command::ToggleTerminal),
    ("Go to where a function or label is defined.", Command::GoToDefinition),
];

/// The starters picked out under New as, by their names in the list of kinds, as long as they're still in it.
const STARTERS: [&str; 4] = ["Spectrum BASIC", "C64 demo (6502)", "Amiga demo (68000, How To Code v8)", "Vectrex cartridge (6809)"];

/// How many of each kind of recent thing are shown before More.
const FEW_FILES: usize = 7;
const FEW_OTHERS: usize = 4;

/// What the start page remembers while it's shown.
#[derive(Default)]
pub(super) struct StartState {
    /// Every recent thing shown, not just the latest few.
    pub more: bool,
    /// The tip shown, counting on from today's; None until the page is first drawn.
    pub tip: Option<usize>,
    /// Which of the recent lists is shown: Projects each time the page is shown afresh.
    tab: super::recent::Tab,
    /// The pass the page was last drawn in, to tell when it's shown afresh.
    drawn: Option<u64>,
}

/// What a click on the start page asks for, done once it's drawn.
enum Asked {
    Command(Command),
    /// A recent file, and the project it was in, to be opened first.
    File(PathBuf, Option<PathBuf>),
    Project(PathBuf),
    Remote(String),
}

/// A link on the start page: a picture, its words, and its keys faintly at the right. Read out as a link, reached
/// by Tab, and followed with Enter.
fn link(ui: &mut Ui, icon: Option<Icon>, words: &str, keys: &str, about: &str) -> egui::Response {
    let width = ui.available_width().min(420.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
    let hot = response.hovered() || response.has_focus();
    if hot {
        ui.painter().rect_filled(rect, 5.0, theme::hover());
    }
    if response.has_focus() {
        ui.painter().rect_stroke(rect, 5.0, Stroke::new(1.5, theme::accent()), egui::StrokeKind::Inside);
    }
    let mut left = rect.min.x + 6.0;
    if let Some(icon) = icon {
        ribbon_icons::paint(ui.painter(), Rect::from_center_size(pos2(left + 9.0, rect.center().y), vec2(18.0, 18.0)), icon, theme::text_dim(), theme::accent(), theme::window());
        left += 28.0;
    }
    let ink = if hot { theme::accent() } else { theme::text() };
    let galley = ui.painter().layout_no_wrap(words.to_string(), egui::FontId::proportional(14.0), ink);
    let text_rect = Rect::from_min_size(pos2(left, rect.center().y - galley.size().y / 2.0), galley.size());
    ui.painter().galley(text_rect.min, galley, ink);
    if response.hovered() {
        ui.painter().line_segment([text_rect.left_bottom(), text_rect.right_bottom()], Stroke::new(1.0, theme::accent()));
    }
    if !keys.is_empty() {
        ui.painter().text(pos2(rect.max.x - 8.0, rect.center().y), egui::Align2::RIGHT_CENTER, keys, egui::FontId::proportional(12.0), theme::text_faint());
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, words));
    let mut said = keys.to_string();
    if !about.is_empty() {
        said = if said.is_empty() { about.to_string() } else { format!("{said}, {about}") };
    }
    crate::access::describe(&response, &said);
    if about.is_empty() { response } else { response.on_hover_text(about) }
}

/// A recent file, folder or remote file: its badge, its name, and where it is, faintly.
fn recent_link(ui: &mut Ui, name: &str, place: &str, badge: Option<&str>, icon: Icon) -> egui::Response {
    let width = ui.available_width().min(460.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 24.0), Sense::click());
    let hot = response.hovered() || response.has_focus();
    if hot {
        ui.painter().rect_filled(rect, 5.0, theme::hover());
    }
    if response.has_focus() {
        ui.painter().rect_stroke(rect, 5.0, Stroke::new(1.5, theme::accent()), egui::StrokeKind::Inside);
    }
    let icon_rect = Rect::from_min_size(pos2(rect.min.x + 6.0, rect.min.y), vec2(18.0, rect.height()));
    match badge {
        Some(file) => crate::file_icons::paint(ui.painter(), icon_rect, file, 13.0),
        None => ribbon_icons::paint(ui.painter(), Rect::from_center_size(icon_rect.center(), vec2(16.0, 16.0)), icon, theme::text_dim(), theme::accent(), theme::window()),
    }
    let left = rect.min.x + 32.0;
    let ink = if hot { theme::accent() } else { theme::text() };
    let galley = ui.painter().layout_no_wrap(name.to_string(), egui::FontId::proportional(13.5), ink);
    let name_width = galley.size().x;
    ui.painter().galley(pos2(left, rect.center().y - galley.size().y / 2.0), galley, ink);
    // Where it is, cut short from its start when there isn't room, as the end says most.
    let room = rect.max.x - (left + name_width + 12.0) - 6.0;
    if room > 40.0 && !place.is_empty() {
        let font = egui::FontId::proportional(12.0);
        let mut shown = place.to_string();
        let fits = |text: &str| ui.painter().layout_no_wrap(text.to_string(), font.clone(), theme::text_faint()).size().x <= room;
        while !fits(&shown) && shown.chars().count() > 4 {
            let rest: String = shown.chars().skip(if shown.starts_with('\u{2026}') { 2 } else { 1 }).collect();
            shown = format!("\u{2026}{rest}");
        }
        ui.painter().text(pos2(left + name_width + 12.0, rect.center().y), egui::Align2::LEFT_CENTER, shown, font, theme::text_faint());
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, name));
    crate::access::describe(&response, place);
    response.on_hover_text(place)
}

/// A section's heading, read out as one.
fn heading(ui: &mut Ui, words: &str) {
    ui.add_space(6.0);
    let response = ui.label(RichText::new(words).size(17.0).color(theme::strong_text()));
    crate::access::named(&response, egui::Role::Heading, words);
    ui.add_space(4.0);
}

/// A smaller heading within a section.
fn subheading(ui: &mut Ui, words: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(words).size(12.0).color(theme::text_faint()));
}

/// The tip to show first: one a day, going round the list.
fn tip_of_the_day() -> usize {
    let days = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400);
    (days % TIPS.len() as u64) as usize
}

impl Limoniter {
    pub(super) fn open_guide(&mut self, page: Guide) {
        self.open_page(&page.tab_name(), guide_markdown(page));
    }

    pub(super) fn open_whats_new(&mut self) {
        let (version, markdown) = whats_new(env!("CARGO_PKG_VERSION"));
        self.open_page(&format!("{}.md", crate::i18n::tf!("What's new in Limoniter {}", version)), markdown);
    }

    /// The Git wizard, set to copy a project from online, as Clone a Git repository asks.
    pub(super) fn open_git_clone(&mut self) {
        self.open_git_wizard();
        if let Some(wizard) = self.git.wizard.as_mut() {
            wizard.start = super::git_wizard::Start::Copy;
        }
    }

    /// Draws the start page in the middle of the window, and does what's clicked.
    pub(super) fn start_page(&mut self, ui: &mut Ui) {
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, theme::window());
        // Files carried over the window, about to be dropped: they open, as anywhere on the window.
        let carried = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let tip = *self.start.tip.get_or_insert_with(tip_of_the_day) % TIPS.len();
        let mut asked = None;
        egui::ScrollArea::vertical().id_salt("start-page").auto_shrink([false, false]).show(ui, |ui| {
            let width = ui.available_width();
            let content = width.min(980.0) - 48.0;
            let side = ((width - content) / 2.0).max(16.0);
            ui.add_space(28.0);
            ui.horizontal(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(content);
                    self.start_header(ui);
                    ui.add_space(18.0);
                    let two = content >= 680.0;
                    if two {
                        ui.columns(2, |columns| {
                            self.start_section(&mut columns[0], &mut asked);
                            self.learn_section(&mut columns[0], &mut asked);
                            self.recent_section(&mut columns[1], &mut asked);
                            self.tip_box(&mut columns[1], tip, &mut asked);
                        });
                    } else {
                        self.start_section(ui, &mut asked);
                        self.recent_section(ui, &mut asked);
                        self.learn_section(ui, &mut asked);
                        self.tip_box(ui, tip, &mut asked);
                    }
                    ui.add_space(20.0);
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut self.view.start_page, crate::i18n::tr("Show this page when every tab is closed")).on_hover_text(crate::i18n::tr("Also in Settings, Appearance. When it's off, the window is left empty."));
                        ui.add_space(16.0);
                        ui.checkbox(&mut self.prefs.reopen_project, crate::i18n::tr("Open the last project when Limoniter starts")).on_hover_text(crate::i18n::tr("Also in Settings, Files. The project that was open when Limoniter last closed opens again, with its files."));
                        ui.add_space(16.0);
                        ui.label(RichText::new(crate::i18n::tr("Files and folders dropped here open.")).color(theme::text_faint()));
                    });
                    ui.add_space(16.0);
                });
            });
        });
        if carried {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("start-drop")));
            painter.rect(full.shrink(8.0), 10.0, theme::accent().gamma_multiply(0.12), Stroke::new(2.0, theme::accent()), egui::StrokeKind::Inside);
            painter.text(full.center(), egui::Align2::CENTER_CENTER, crate::i18n::tr("Drop to open"), egui::FontId::proportional(22.0), theme::accent());
        }
        match asked {
            Some(Asked::Command(command)) => self.pending_command = Some(command),
            Some(Asked::File(path, project)) => self.open_recent_file(path, project),
            Some(Asked::Project(path)) => self.open_project(&path),
            Some(Asked::Remote(address)) => {
                self.open_sftp_address(&address);
            }
            None => {}
        }
        if self.pending_command.is_some() || self.open_request.is_some() {
            ui.ctx().request_repaint();
        }
    }

    fn start_header(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // The About box's logo: a picture with no name of its own, as the heading beside it says it.
            if let Some(logo) = about_logo(ui.ctx()) {
                ui.add(egui::Image::new(&logo).fit_to_exact_size(vec2(56.0, 56.0)));
                ui.add_space(8.0);
            }
            ui.vertical(|ui| {
                let title = ui.label(RichText::new(crate::i18n::tr("Limoniter")).size(30.0).color(theme::strong_text()));
                crate::access::named(&title, egui::Role::Heading, crate::i18n::tr("Limoniter"));
                ui.label(RichText::new(crate::i18n::tf!("Text and hex editing, from today's code to yesterday's computers. Version {}.", env!("CARGO_PKG_VERSION"))).color(theme::text_dim()));
            });
        });
    }

    fn start_section(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        let keys = |command: Command| keys_for_command(command).map(|(_, keys)| keys).unwrap_or_default();
        heading(ui, crate::i18n::tr("Start"));
        if link(ui, Some(Icon::New), crate::i18n::tr("New file"), &keys(Command::New), "").clicked() {
            *asked = Some(Asked::Command(Command::New));
        }
        // New as: the retro starters by name, and every kind in a list, lined up under New file's words.
        ui.horizontal(|ui| {
            ui.add_space(34.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(crate::i18n::tr("New as:")).color(theme::text_dim()));
                for name in STARTERS {
                    if let Some(index) = self.new_kinds.iter().position(|kind| kind.name == name) {
                        let shown = self.new_kinds[index].name.clone();
                        let response = ui.link(RichText::new(&shown).color(theme::accent())).on_hover_text(crate::i18n::tf!("A new {} file, with a starter to change", shown));
                        if response.clicked() {
                            *asked = Some(Asked::Command(Command::NewAs(index)));
                        }
                    }
                }
                let more = crate::fit::menu_button(ui, RichText::new(crate::i18n::tr("Every kind\u{2026}")).color(theme::accent()), |ui| {
                    if let Some(command) = self.drop_down_list(ui, &[menus::Entry::Live("New as", menus::Live::NewAs)]) {
                        *asked = Some(Asked::Command(command));
                    }
                });
                crate::access::rename(&more.response, crate::i18n::tr("Every kind of new file"));
            });
        });
        ui.add_space(4.0);
        let rows = [
            (Icon::Open, "Open a file\u{2026}", Command::Open, ""),
            (Icon::Open, "Open a folder\u{2026}", Command::OpenFolder, "Work on a whole folder of code, with the Explorer, Go to file and the Search panel"),
            (Icon::Server, "Open a remote file or folder\u{2026}", Command::OpenRemote, "On a server you reach by SFTP"),
            (Icon::Git, "Clone a Git repository\u{2026}", Command::GitClone, "Copy a project from GitHub or elsewhere, with the Git wizard"),
            (Icon::Web, "Open from the web\u{2026}", Command::OpenUrl, "A file at an http or https address"),
            (Icon::Disc, "New disk or tape image\u{2026}", Command::NewImage, "For the Amiga, Atari, Commodore, Spectrum and others"),
        ];
        for (icon, words, command, about) in rows {
            if link(ui, Some(icon), crate::i18n::tr(words), &keys(command), crate::i18n::tr(about)).clicked() {
                *asked = Some(Asked::Command(command));
            }
        }
    }

    fn learn_section(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        ui.add_space(14.0);
        heading(ui, crate::i18n::tr("Learn"));
        for page in Guide::ALL {
            if link(ui, Some(Icon::Book), crate::i18n::tr(page.title()), "", "").clicked() {
                *asked = Some(Asked::Command(Command::Guide(page)));
            }
        }
        if link(ui, Some(Icon::Book), crate::i18n::tr("How things work"), "", crate::i18n::tr("Everything in short, a section for each kind of work")).clicked() {
            *asked = Some(Asked::Command(Command::Help));
        }
        if link(ui, Some(Icon::Sprite), crate::i18n::tr("How To Code: writing Amiga demos"), "", "").clicked() {
            *asked = Some(Asked::Command(Command::HowToCode));
        }
        let (version, _) = whats_new(env!("CARGO_PKG_VERSION"));
        if link(ui, Some(Icon::Star), &crate::i18n::tf!("What's new in {}", version), "", "").clicked() {
            *asked = Some(Asked::Command(Command::WhatsNew));
        }
    }

    fn recent_section(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        heading(ui, crate::i18n::tr("Recent"));
        // Projects each time the page is shown afresh, as File, Open recent does.
        let pass = ui.ctx().cumulative_pass_nr();
        if self.start.drawn.is_none_or(|drawn| drawn + 1 < pass) {
            self.start.tab = self.recent_lists.opens_on;
        }
        self.start.drawn = Some(pass);
        let more = self.start.more;
        let few = |count: usize| if more { usize::MAX } else { count };
        let mut remote: Vec<String> = self.remote_recent.clone();
        for site in &self.remote_sites {
            let url = site.site.url("");
            if !remote.contains(&url) {
                remote.push(url);
            }
        }
        if self.panels.recent.is_empty() && self.recent_projects.is_empty() && remote.is_empty() {
            ui.label(RichText::new(crate::i18n::tr("Files, folders and projects you open will be listed here, each a click away.")).color(theme::text_faint()));
            return;
        }
        ui.horizontal(|ui| {
            for (tab, words) in [(super::recent::Tab::Projects, crate::i18n::tr("Projects")), (super::recent::Tab::Files, crate::i18n::tr("Files"))] {
                ui.selectable_value(&mut self.start.tab, tab, RichText::new(words).size(13.5));
            }
        });
        let name_of = |path: &Path| path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
        let empty = |ui: &mut Ui, words: &str| {
            ui.add_space(4.0);
            ui.label(RichText::new(words).color(theme::text_faint()));
        };
        let cut = match self.start.tab {
            super::recent::Tab::Projects => {
                let projects: Vec<PathBuf> = self.recent_projects.iter().take(few(FEW_OTHERS)).cloned().collect();
                if projects.is_empty() {
                    empty(ui, crate::i18n::tr("Projects you open will be listed here."));
                }
                for path in &projects {
                    let (name, place, folder) = self.recent_lists.shown_as(path);
                    if recent_link(ui, &name, &place, None, if folder { Icon::Open } else { Icon::Book }).clicked() {
                        *asked = Some(Asked::Project(path.clone()));
                    }
                }
                projects.len() < self.recent_projects.len()
            }
            super::recent::Tab::Files => {
                let shown = self.panels.recent.len().min(few(FEW_FILES));
                if shown == 0 {
                    empty(ui, crate::i18n::tr("Files you open will be listed here."));
                }
                for group in super::recent::grouped(&self.panels.recent[..shown], &self.recent_in) {
                    subheading(ui, &self.recent_lists.group_name(&group));
                    for path in group.files {
                        let place = path.parent().map_or_else(String::new, |p| p.display().to_string());
                        let name = name_of(&path);
                        if recent_link(ui, &name, &place, Some(&name), Icon::New).clicked() {
                            *asked = Some(Asked::File(path, group.project.clone()));
                        }
                    }
                }
                shown < self.panels.recent.len()
            }
        } || remote.len() > FEW_OTHERS;
        remote.truncate(few(FEW_OTHERS));
        if !remote.is_empty() {
            subheading(ui, crate::i18n::tr("Remote"));
            for address in remote {
                let name = address.trim_end_matches('/').rsplit('/').next().filter(|n| !n.is_empty() && !n.contains('@')).map_or_else(|| address.clone(), str::to_string);
                if recent_link(ui, &name, &address, None, Icon::Server).clicked() {
                    *asked = Some(Asked::Remote(address));
                }
            }
        }
        if cut || more {
            ui.add_space(2.0);
            let words = if more { crate::i18n::tr("Fewer") } else { crate::i18n::tr("More\u{2026}") };
            if link(ui, None, words, "", if more { "" } else { crate::i18n::tr("Every recent file, folder and remote file") }).clicked() {
                self.start.more = !more;
            }
        }
    }

    fn tip_box(&mut self, ui: &mut Ui, tip: usize, asked: &mut Option<Asked>) {
        ui.add_space(14.0);
        let (words, command) = TIPS[tip];
        let keys = keys_for_command(command).map(|(_, keys)| keys).unwrap_or_default();
        egui::Frame::new().fill(theme::field()).stroke(Stroke::new(1.0, theme::border())).corner_radius(8.0).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.set_width(ui.available_width().min(460.0));
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                ribbon_icons::paint(ui.painter(), rect, Icon::Bulb, theme::text_dim(), theme::accent(), theme::field());
                ui.label(RichText::new(crate::i18n::tr("Tip")).strong().color(theme::accent()));
            });
            ui.label(RichText::new(crate::i18n::tr(words)).color(theme::text()));
            ui.horizontal(|ui| {
                if !keys.is_empty() {
                    let chip = egui::Frame::new().fill(theme::window()).stroke(Stroke::new(1.0, theme::border())).corner_radius(4.0).inner_margin(egui::Margin::symmetric(6, 2));
                    chip.show(ui, |ui| ui.label(RichText::new(&keys).monospace().color(theme::strong_text())));
                }
                if ui.link(crate::i18n::tr("Try it")).clicked() {
                    *asked = Some(Asked::Command(command));
                }
                if ui.link(crate::i18n::tr("Another tip")).clicked() {
                    self.start.tip = Some(tip + 1);
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{app_with, draw};
    use super::*;
    use crate::access::tests::{find, listing, nodes};
    use eframe::egui::accesskit::Role;

    #[test]
    fn whats_new_is_this_versions_notes_or_the_newest() {
        let (version, page) = whats_new("0.13.0");
        assert_eq!(version, "0.13.0");
        assert!(page.starts_with("# What's new in Limoniter 0.13.0\n"));
        assert!(page.contains("Released 18 November 2026."));
        assert!(!page.contains("## 0.12"), "only the one version's notes");
        let (newest, _) = whats_new("99.0.0");
        let first = WHATS_NEW.split("\n## ").nth(1).and_then(|s| s.lines().next()).unwrap();
        assert_eq!(newest, first.trim());
    }

    #[test]
    fn every_guide_page_has_a_title_headings_and_points() {
        for page in Guide::ALL {
            let markdown = guide_markdown(page);
            assert!(markdown.starts_with(&format!("# {}\n", page.title())));
            assert!(markdown.matches("\n## ").count() >= 3, "{page:?}");
            assert!(markdown.contains("\n- "), "{page:?}");
        }
    }

    #[test]
    fn every_tip_has_keys_from_the_menus() {
        for (words, command) in TIPS {
            assert!(keys_for_command(command).is_some(), "{words}");
        }
    }

    #[test]
    fn the_starters_named_are_in_the_usual_list_of_kinds() {
        let kinds = new_kinds::NewKind::usual();
        for name in STARTERS {
            assert!(kinds.iter().any(|kind| kind.name == name), "{name}");
        }
    }

    fn start_page_app() -> (Limoniter, Context) {
        let (mut app, ctx) = app_with("");
        app.close_tab(0);
        assert!(app.no_file());
        app.panels.recent = vec![PathBuf::from("/work/game/main.asm"), PathBuf::from("/work/notes.md")];
        app.remote_recent = vec!["sftp://me@example.org/home/me/site/index.html".into()];
        ctx.enable_accesskit();
        (app, ctx)
    }

    #[test]
    fn the_start_page_shows_once_every_tab_is_closed_with_every_link_named() {
        let (mut app, ctx) = start_page_app();
        draw(&mut app, &ctx, Vec::new());
        let output = nodes(&draw(&mut app, &ctx, Vec::new()));
        for name in ["New file", "Open a file\u{2026}", "Open a folder\u{2026}", "Open a remote file or folder\u{2026}", "Clone a Git repository\u{2026}", "Open from the web\u{2026}", "New disk or tape image\u{2026}", "Getting started", "Working on a folder of code", "Assembling and running", "index.html"] {
            let found = find(&output, Role::Link, name);
            assert!(found.is_some(), "no link named {name}: {}", listing(&output));
        }
        assert!(find(&output, Role::Heading, "Recent").is_some(), "{}", listing(&output));
        // The recent projects are shown first; the files are on the other tab.
        assert!(find(&output, Role::Link, "main.asm").is_none(), "{}", listing(&output));
        app.start.tab = super::super::recent::Tab::Files;
        let output = nodes(&draw(&mut app, &ctx, Vec::new()));
        for name in ["main.asm", "notes.md", "index.html"] {
            assert!(find(&output, Role::Link, name).is_some(), "no link named {name}: {}", listing(&output));
        }
        // Turned off, the window is empty, as it used to be.
        app.view.start_page = false;
        let output = nodes(&draw(&mut app, &ctx, Vec::new()));
        assert!(find(&output, Role::Link, "New file").is_none());
    }

    #[test]
    fn the_start_page_shows_projects_first_and_the_files_grouped_by_project() {
        let (mut app, ctx) = start_page_app();
        let dir = std::env::temp_dir().join(format!("limoniter-start-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut project = crate::project::Project::new(&dir.join("game.limproj"));
        project.name = "Space game".into();
        project.save().unwrap();
        app.recent_projects = vec![project.path.clone()];
        app.recent_in.note(Path::new("/work/game/main.asm"), Some(&project.path));
        draw(&mut app, &ctx, Vec::new());
        let output = nodes(&draw(&mut app, &ctx, Vec::new()));
        assert!(find(&output, Role::Link, "Space game").is_some(), "the project, by its name: {}", listing(&output));
        assert!(find(&output, Role::Link, "main.asm").is_none());
        // On the Files tab, the project's file is under its name, newest group first, and the other on its own.
        app.start.tab = super::super::recent::Tab::Files;
        let output = nodes(&draw(&mut app, &ctx, Vec::new()));
        let mut order: Vec<(f64, String)> = output.iter().filter_map(|(_, node)| Some((node.bounds()?.y0, node.label().or(node.value())?.to_string()))).filter(|(_, name)| ["Space game", "main.asm", "On their own", "notes.md"].contains(&name.as_str())).collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        order.dedup_by(|a, b| a.1 == b.1);
        assert_eq!(order.into_iter().map(|(_, name)| name).collect::<Vec<_>>(), ["Space game", "main.asm", "On their own", "notes.md"], "{}", listing(&output));
        // Shown afresh, it's back on Projects.
        app.new_document();
        draw(&mut app, &ctx, Vec::new());
        app.close_tab(app.active);
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(app.start.tab, super::super::recent::Tab::Projects);
        drop(app);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_guide_whats_new_and_clone_open_as_they_should() {
        let (mut app, _ctx) = start_page_app();
        app.open_guide(Guide::Folder);
        assert!(!app.no_file());
        assert_eq!(app.docs[app.active].untitled, "Working on a folder of code.md");
        assert_eq!(app.docs[app.active].preview_view, settings::PreviewView::Full, "shown as the page it makes");
        app.open_guide(Guide::Folder);
        assert_eq!(app.docs.len(), 1, "opened again, it's gone to rather than opened twice");
        app.close_tab(app.active);
        app.open_whats_new();
        assert!(app.docs[app.active].text.line(0).starts_with("# What's new in Limoniter"));
        app.close_tab(app.active);
        app.open_git_clone();
        assert_eq!(app.git.wizard.as_ref().map(|w| w.start), Some(super::super::git_wizard::Start::Copy));
    }

    #[test]
    fn tab_and_enter_reach_and_follow_the_links() {
        let (mut app, ctx) = start_page_app();
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, Vec::new());
        let tab = egui::Event::Key { key: egui::Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        let enter = egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        // Tab goes from link to link until it reaches New file; Enter makes a new file.
        let mut reached = false;
        let mut went = Vec::new();
        for _ in 0..40 {
            let output = draw(&mut app, &ctx, vec![tab.clone()]);
            let focused = output.platform_output.accesskit_update.as_ref().and_then(|u| u.nodes.iter().find(|(id, _)| *id == u.focus).map(|(id, n)| format!("{:?} {} {:?} {:?}", n.role(), n.label().unwrap_or_default(), id, n.bounds())));
            went.push(focused.clone().unwrap_or_default());
            if focused.as_deref().is_some_and(|f| f.starts_with("Link New file ")) {
                reached = true;
                break;
            }
        }
        assert!(reached, "Tab never reached New file: {went:?}");
        draw(&mut app, &ctx, vec![enter]);
        draw(&mut app, &ctx, Vec::new());
        assert!(!app.no_file(), "Enter on New file made one");
    }

    #[test]
    fn the_start_page_draws_in_both_themes_wide_and_narrow() {
        for light in [true, false] {
            theme::set_light(light);
            let (mut app, ctx) = start_page_app();
            for width in [1280.0, 600.0, 320.0] {
                let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 480.0))), ..Default::default() };
                let mut output = ctx.run_ui(raw, |ui| app.draw_frame(ui));
                output.textures_delta.clear();
            }
        }
        theme::set_light(false);
    }
}
