//! The preview beside the text. Markdown is the first format; another joins by adding a `Kind`, saying which
//! files it's for in `kind_for`, and turning its text into `Block`s in `render`.
//! Nothing here runs anything or fetches anything: links only open when clicked, and images aren't loaded.

use crate::theme;
use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke, TextFormat, Ui, text::LayoutJob};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::path::Path;

/// Whether a true bold font was found. The fonts that come with the app have none, so it borrows Windows' own.
static HAS_BOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const BOLD_FAMILY: &str = "preview-bold";

/// Adds Windows' bold font for the preview to use, if it's there. Without it, bold shows as a stronger color alone.
pub fn add_bold_font(fonts: &mut egui::FontDefinitions) {
    let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\segoeuib.ttf") else {
        return;
    };
    fonts.font_data.insert(BOLD_FAMILY.into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
    let mut family = vec![BOLD_FAMILY.to_string()];
    family.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(FontFamily::Name(BOLD_FAMILY.into()), family);
    HAS_BOLD.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// The most text a preview will lay out. It's redone as you type, so a huge file would make typing slow.
pub const MOST_PREVIEW_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Markdown,
    /// PC ANSI art, with its SAUCE record; BinaryText too when SAUCE says that's what it is.
    AnsiArt,
    /// A Commodore PETSCII stream, as PRINTing it would show it.
    Petscii,
    /// Commodore screen memory.
    ScreenCodes,
    /// Teletext rows, or BBC Mode 7 screen memory.
    Teletext,
    /// ZX Spectrum text, with its colour and position controls.
    Spectrum,
    /// A tokenised BASIC program, listed as the machine's LIST would show it.
    Basic,
    /// A picture in an older machine's format, such as an Amiga IFF ILBM or a Spectrum screen.
    Picture,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Markdown => "Markdown",
            Kind::AnsiArt => "ANSI art",
            Kind::Petscii => "PETSCII screen",
            Kind::ScreenCodes => "Commodore screen",
            Kind::Teletext => "Teletext",
            Kind::Spectrum => "ZX Spectrum screen",
            Kind::Basic => "BASIC listing",
            Kind::Picture => "Picture",
        }
    }

    /// Drawn as a machine's screen, rather than laid out as a document.
    pub fn is_screen(self) -> bool {
        !matches!(self, Kind::Markdown | Kind::Basic | Kind::Picture)
    }

    /// Made from the file's exact bytes rather than its text, so it works from hex mode too.
    pub fn reads_bytes(self) -> bool {
        self != Kind::Markdown
    }
}

/// What a preview made from bytes came out as.
pub enum FromBytes {
    Screen(crate::screen::Screen),
    Blocks(Vec<Block>),
    Picture(crate::pictures::Picture),
    /// Nothing to show, and why.
    Nothing(String),
}

/// Which preview suits a file, going by its name, the language it's viewed as, and how its bytes are read.
/// Retro screens are offered by file ending, by the display chosen for the file, or by its character set.
pub fn kind_for_file(path: Option<&Path>, language: &str, format: &crate::codec::TextFormat) -> Option<Kind> {
    if let Some(kind) = kind_for(path, language) {
        return Some(kind);
    }
    use crate::codec::Display;
    let extension = path.and_then(Path::extension).and_then(|e| e.to_str()).map(str::to_lowercase);
    let id = format.charset.id.as_str();
    if extension.as_deref().is_some_and(|e| crate::basic::is_program_file(e, None)) {
        return Some(Kind::Basic);
    }
    match (extension.as_deref(), format.display) {
        (_, Display::AnsiArt | Display::AmigaConsole) | (Some("ans" | "ice" | "diz" | "nfo" | "xb" | "pcb" | "avt"), _) => Some(Kind::AnsiArt),
        (_, Display::Petscii) | (Some("seq"), _) => Some(Kind::Petscii),
        (_, Display::Teletext | Display::Mode7) | (Some("ep1" | "tti" | "ttix"), _) => Some(Kind::Teletext),
        _ if id.starts_with("petscii-") => Some(Kind::Petscii),
        _ if id.starts_with("screen-") => Some(Kind::ScreenCodes),
        _ if id.starts_with("teletext-") || id == "bbc-mode7" => Some(Kind::Teletext),
        _ if id == "zx-spectrum" => Some(Kind::Spectrum),
        _ => None,
    }
}

/// Draws a file's bytes as the screen a preview of this kind shows.
pub fn render_screen(kind: Kind, bytes: &[u8], format: &crate::codec::TextFormat) -> crate::screen::Screen {
    let set = format.charset;
    // Art is drawn in the DOS set it was made in, whatever the file is being edited as, unless that's a DOS set too.
    let dos = if set.id.starts_with("cp") { set } else { crate::charset::known("cp437") };
    match kind {
        Kind::Petscii => {
            let set = if set.id.starts_with("petscii-") { set } else { crate::charset::known("petscii-c64") };
            crate::retro_screens::petscii(bytes, set, format.state)
        }
        Kind::ScreenCodes => crate::retro_screens::screen_codes(bytes, set, format.state),
        Kind::Spectrum => crate::retro_screens::spectrum(bytes, crate::charset::known("zx-spectrum")),
        Kind::Teletext => {
            let mode7 = set.id == "bbc-mode7" || format.display == crate::codec::Display::Mode7;
            let set = if set.id.starts_with("teletext-") || set.id == "bbc-mode7" { set } else { crate::charset::known("teletext-en") };
            if crate::retro_screens::looks_like_tti(bytes) {
                return crate::retro_screens::tti(bytes, set);
            }
            crate::retro_screens::teletext(bytes, set, format.newline.parity(), mode7)
        }
        Kind::AnsiArt if is_amiga_console(format) => crate::ansi_art::render_amiga(bytes),
        Kind::AnsiArt | Kind::Markdown | Kind::Basic | Kind::Picture => crate::ansi_art::render(bytes, dos),
    }
}

/// Whether text is shown as the Amiga's console shows it, in its pens.
pub fn is_amiga_console(format: &crate::codec::TextFormat) -> bool {
    format.display == crate::codec::Display::AmigaConsole || format.charset.id == "amiga-text"
}

/// Makes a preview from a file's bytes: a retro screen, or a BASIC program's listing.
pub fn render_bytes(kind: Kind, bytes: &[u8], format: &crate::codec::TextFormat, path: Option<&Path>) -> FromBytes {
    if kind == Kind::Picture {
        let name = path.and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        return match crate::pictures::detect(&name, bytes).ok_or_else(|| "This isn't a picture Limoniter can show any more.".to_string()).and_then(|f| crate::pictures::decode_named(f, &name, bytes)) {
            Ok(picture) => FromBytes::Picture(picture),
            Err(why) => FromBytes::Nothing(why),
        };
    }
    if kind != Kind::Basic {
        return FromBytes::Screen(render_screen(kind, bytes, format));
    }
    let extension = path.and_then(Path::extension).and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    if !crate::basic::is_program_file(&extension, Some(bytes)) {
        return FromBytes::Nothing("This .bas file is a listing kept as text, not a tokenised program. Format, Tokenised BASIC can save it as one.".to_string());
    }
    FromBytes::Blocks(listing_blocks(&crate::basic::list(bytes, &extension, format.charset)))
}

/// A listing as a document: what the file says about itself, any notes, then the program.
pub fn listing_blocks(listing: &crate::basic::Listing) -> Vec<Block> {
    let paragraph = |spans: Vec<Span>| Block { kind: BlockKind::Paragraph, spans, indent: 0, quote: 0, marker: None };
    let mut blocks = vec![paragraph(vec![Span { text: listing.dialect.to_string(), strong: true, ..Span::default() }])];
    for (key, value) in &listing.info {
        blocks.push(paragraph(vec![Span { text: format!("{key}: "), strong: true, ..Span::default() }, Span { text: value.clone(), ..Span::default() }]));
    }
    for note in &listing.notes {
        blocks.push(paragraph(vec![Span { text: note.clone(), italic: true, ..Span::default() }]));
    }
    if !listing.lines.is_empty() {
        blocks.push(Block { kind: BlockKind::Code, spans: vec![Span { text: listing.lines.join("\n"), code: true, ..Span::default() }], indent: 0, quote: 0, marker: None });
    }
    if !listing.screen.is_empty() {
        blocks.push(paragraph(vec![Span { text: "The screen when it was saved".into(), strong: true, ..Span::default() }]));
        blocks.push(Block { kind: BlockKind::Code, spans: vec![Span { text: listing.screen.join("\n"), code: true, ..Span::default() }], indent: 0, quote: 0, marker: None });
    }
    blocks
}

/// Which preview suits a file, going by its name and the language it's being viewed as.
pub fn kind_for(path: Option<&Path>, language: &str) -> Option<Kind> {
    let extension = path.and_then(Path::extension).and_then(|e| e.to_str()).map(str::to_lowercase);
    match extension.as_deref() {
        Some("md" | "markdown" | "mdown" | "mkd" | "mdtext") => Some(Kind::Markdown),
        _ if matches!(language, "Markdown" | "MultiMarkdown") => Some(Kind::Markdown),
        _ => None,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub strong: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    /// Only for addresses that are safe to hand to the browser.
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Heading(u8),
    Paragraph,
    Code,
    Rule,
    Table(Vec<Vec<Vec<Span>>>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub spans: Vec<Span>,
    /// How many lists this is inside.
    pub indent: usize,
    /// How many quotes this is inside.
    pub quote: usize,
    /// A bullet or number, on the first paragraph of a list item.
    pub marker: Option<String>,
}

/// Turns a file's text into what's drawn.
pub fn render(kind: Kind, text: &str) -> Vec<Block> {
    match kind {
        Kind::Markdown => markdown(text),
        _ => Vec::new(),
    }
}

/// Only these are handed to the browser. Anything else, such as a script or a file on this computer, stays plain text.
fn safe_link(address: &str) -> Option<String> {
    let lower = address.trim().to_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")).then(|| address.trim().to_string())
}

#[derive(Default)]
struct Builder {
    blocks: Vec<Block>,
    spans: Vec<Span>,
    strong: usize,
    italic: usize,
    strike: usize,
    link: Option<String>,
    quote: usize,
    /// For each list we're inside, the next number, or None if it has bullets.
    lists: Vec<Option<u64>>,
    marker: Option<String>,
    in_code: bool,
    table: Option<Vec<Vec<Vec<Span>>>>,
    row: Vec<Vec<Span>>,
}

impl Builder {
    fn push_text(&mut self, text: &str, code: bool) {
        self.spans.push(Span { text: text.to_string(), strong: self.strong > 0, italic: self.italic > 0, code, strike: self.strike > 0, link: self.link.clone() });
    }

    fn push_block(&mut self, kind: BlockKind) {
        let spans = std::mem::take(&mut self.spans);
        self.blocks.push(Block { kind, spans, indent: self.lists.len(), quote: self.quote, marker: self.marker.take() });
    }

    /// Ends the text so far, if there is any. An item with nothing in it still gets its bullet.
    fn flush(&mut self) {
        if !self.spans.is_empty() || self.marker.is_some() {
            self.push_block(BlockKind::Paragraph);
        }
    }
}

fn markdown(text: &str) -> Vec<Block> {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut b = Builder::default();
    for event in Parser::new_ext(text, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Strong => b.strong += 1,
                Tag::Emphasis => b.italic += 1,
                Tag::Strikethrough => b.strike += 1,
                Tag::Link { dest_url, .. } => b.link = safe_link(&dest_url),
                Tag::Image { .. } => b.push_text("[image: ", false),
                Tag::BlockQuote(_) => {
                    b.flush();
                    b.quote += 1;
                }
                Tag::List(start) => {
                    b.flush();
                    b.lists.push(start);
                }
                Tag::Item => {
                    b.flush();
                    let marker = match b.lists.last_mut() {
                        Some(Some(number)) => {
                            *number += 1;
                            format!("{}.", *number - 1)
                        }
                        _ => "\u{2022}".to_string(),
                    };
                    b.marker = Some(marker);
                }
                Tag::CodeBlock(_) => {
                    b.flush();
                    b.in_code = true;
                }
                Tag::Table(_) => {
                    b.flush();
                    b.table = Some(Vec::new());
                }
                Tag::TableHead | Tag::TableRow => b.row = Vec::new(),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Strong => b.strong = b.strong.saturating_sub(1),
                TagEnd::Emphasis => b.italic = b.italic.saturating_sub(1),
                TagEnd::Strikethrough => b.strike = b.strike.saturating_sub(1),
                TagEnd::Link => b.link = None,
                TagEnd::Image => b.push_text("]", false),
                TagEnd::Paragraph | TagEnd::Item => b.flush(),
                TagEnd::BlockQuote(_) => {
                    b.flush();
                    b.quote = b.quote.saturating_sub(1);
                }
                TagEnd::List(_) => {
                    b.flush();
                    b.lists.pop();
                }
                TagEnd::Heading(level) => {
                    let number = match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    };
                    b.push_block(BlockKind::Heading(number));
                }
                TagEnd::CodeBlock => {
                    b.in_code = false;
                    b.push_block(BlockKind::Code);
                }
                TagEnd::TableCell => {
                    let cell = std::mem::take(&mut b.spans);
                    b.row.push(cell);
                }
                TagEnd::TableHead | TagEnd::TableRow => {
                    let row = std::mem::take(&mut b.row);
                    if let Some(table) = &mut b.table {
                        table.push(row);
                    }
                }
                TagEnd::Table => {
                    let rows = b.table.take().unwrap_or_default();
                    b.push_block(BlockKind::Table(rows));
                }
                _ => {}
            },
            Event::Text(text) => {
                let in_code = b.in_code;
                b.push_text(&text, in_code);
            }
            Event::Code(text) => b.push_text(&text, true),
            // Web page markup is shown as it's written: it's never acted on.
            Event::Html(text) | Event::InlineHtml(text) => b.push_text(&text, true),
            Event::SoftBreak => b.push_text(" ", false),
            Event::HardBreak => b.push_text("\n", false),
            Event::Rule => {
                b.flush();
                b.push_block(BlockKind::Rule);
            }
            Event::TaskListMarker(done) => b.push_text(if done { "\u{2611} " } else { "\u{2610} " }, false),
            _ => {}
        }
    }
    b.flush();
    b.blocks
}

fn heading_size(level: u8) -> f32 {
    match level {
        1 => 24.0,
        2 => 20.0,
        3 => 17.0,
        _ => 15.0,
    }
}

fn job_for(spans: &[Span], size: f32, heading: bool, width: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    for span in spans {
        let bold = (span.strong || heading) && HAS_BOLD.load(std::sync::atomic::Ordering::Relaxed);
        let family = if span.code {
            FontFamily::Monospace
        } else if bold {
            FontFamily::Name(BOLD_FAMILY.into())
        } else {
            FontFamily::Proportional
        };
        let color: Color32 = if span.link.is_some() {
            theme::accent()
        } else if span.strong || heading {
            theme::strong_text()
        } else {
            theme::text()
        };
        let format = TextFormat {
            font_id: FontId::new(if span.code { size - 1.0 } else { size }, family),
            color,
            italics: span.italic,
            underline: if span.link.is_some() { Stroke::new(1.0, color) } else { Stroke::NONE },
            strikethrough: if span.strike { Stroke::new(1.0, color) } else { Stroke::NONE },
            background: if span.code { theme::ruler_back() } else { Color32::TRANSPARENT },
            ..TextFormat::default()
        };
        job.append(&span.text, 0.0, format);
    }
    job
}

/// The link at a place in the text, counting characters.
fn link_at(spans: &[Span], character: usize) -> Option<&str> {
    let mut count = 0;
    for span in spans {
        count += span.text.chars().count();
        if character < count {
            return span.link.as_deref();
        }
    }
    None
}

fn draw_spans(ui: &mut Ui, spans: &[Span], size: f32, heading: bool) {
    let job = job_for(spans, size, heading, ui.available_width());
    let (position, galley, response) = egui::Label::new(job).sense(egui::Sense::click()).layout_in_ui(ui);
    ui.painter().galley(position, galley.clone(), theme::text());
    follow_links(ui, response, position, &galley, spans);
}

/// Shows and follows a link under the pointer.
fn follow_links(ui: &Ui, response: egui::Response, position: egui::Pos2, galley: &egui::Galley, spans: &[Span]) {
    if let Some(pointer) = response.hover_pos() {
        let cursor = galley.cursor_from_pos(pointer - position);
        if let Some(address) = link_at(spans, cursor.index.into()) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            let address = address.to_string();
            let clicked = response.clicked();
            response.on_hover_text(address.clone());
            if clicked {
                ui.ctx().open_url(egui::OpenUrl::new_tab(address));
            }
        }
    }
}

const TABLE_GAP: f32 = 12.0;

/// How wide each column of a table can be. If they all fit they keep their natural widths. If not, narrow
/// columns still keep theirs and the wide ones share what's left equally, wrapping their text to fit.
/// The result always fits, however little room there is.
fn column_widths(natural: &[f32], available: f32) -> Vec<f32> {
    let gaps = TABLE_GAP * natural.len().saturating_sub(1) as f32;
    let room = (available - gaps).max(natural.len() as f32);
    if natural.iter().sum::<f32>() <= room {
        return natural.to_vec();
    }
    let mut widths = vec![None; natural.len()];
    let mut left = room;
    loop {
        let open = widths.iter().filter(|width: &&Option<f32>| width.is_none()).count();
        if open == 0 {
            break;
        }
        let share = left / open as f32;
        let mut settled = false;
        for (width, wanted) in widths.iter_mut().zip(natural) {
            if width.is_none() && *wanted <= share {
                *width = Some(*wanted);
                left -= *wanted;
                settled = true;
            }
        }
        if !settled {
            for width in widths.iter_mut().filter(|width| width.is_none()) {
                *width = Some(share);
            }
            break;
        }
    }
    widths.into_iter().flatten().collect()
}

/// Draws a table across the room there is. A row is as tall as its tallest cell.
fn draw_table(ui: &mut Ui, rows: &[Vec<Vec<Span>>]) {
    const SIZE: f32 = 13.5;
    const ROW_MARGIN: f32 = 3.0;
    let table_id = ui.next_auto_id();
    let rows: Vec<Vec<Vec<Span>>> = rows
        .iter()
        .enumerate()
        .map(|(number, row)| {
            let mut row = row.clone();
            if number == 0 {
                row.iter_mut().flatten().for_each(|span| span.strong = true);
            }
            row
        })
        .collect();
    let layout = |ui: &Ui, spans: &[Span], width: f32| ui.ctx().fonts_mut(|fonts| fonts.layout_job(job_for(spans, SIZE, false, width)));
    let mut natural = vec![0.0_f32; rows.iter().map(Vec::len).max().unwrap_or(0)];
    for row in &rows {
        for (cell, width) in row.iter().zip(&mut natural) {
            *width = width.max(layout(ui, cell, f32::INFINITY).size().x.ceil());
        }
    }
    let available = ui.available_width();
    let widths = column_widths(&natural, available);
    for (number, row) in rows.iter().enumerate() {
        let galleys: Vec<_> = row.iter().zip(&widths).map(|(cell, width)| layout(ui, cell, *width)).collect();
        let tallest = galleys.iter().map(|galley| galley.size().y).fold(0.0, f32::max);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(available, tallest + ROW_MARGIN * 2.0), egui::Sense::hover());
        if number % 2 == 1 {
            ui.painter().rect_filled(rect, 0.0, theme::ruler_back());
        }
        let mut x = rect.min.x;
        for (place, ((galley, width), cell)) in galleys.into_iter().zip(&widths).zip(row).enumerate() {
            let position = egui::pos2(x, rect.min.y + ROW_MARGIN);
            ui.painter().galley(position, galley.clone(), theme::text());
            if cell.iter().any(|span| span.link.is_some()) {
                let area = egui::Rect::from_min_size(position, galley.size());
                let response = ui.interact(area, table_id.with((number, place)), egui::Sense::click());
                follow_links(ui, response, position, &galley, cell);
            }
            x += width + TABLE_GAP;
        }
    }
}

fn draw_block(ui: &mut Ui, block: &Block) {
    match &block.kind {
        BlockKind::Heading(level) => {
            ui.add_space(8.0);
            draw_spans(ui, &block.spans, heading_size(*level), true);
            if *level <= 2 {
                ui.separator();
            }
        }
        BlockKind::Paragraph => {
            ui.horizontal_top(|ui| {
                if let Some(marker) = &block.marker {
                    ui.label(RichText::new(marker).color(theme::text_dim()));
                }
                ui.vertical(|ui| draw_spans(ui, &block.spans, 14.0, false));
            });
            ui.add_space(4.0);
        }
        BlockKind::Code => {
            egui::Frame::new().fill(theme::ruler_back()).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let text: String = block.spans.iter().map(|span| span.text.as_str()).collect();
                ui.label(RichText::new(text.trim_end_matches('\n')).monospace().size(13.0));
            });
            ui.add_space(6.0);
        }
        BlockKind::Rule => {
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
        }
        BlockKind::Table(rows) => {
            draw_table(ui, rows);
            ui.add_space(6.0);
        }
    }
}

/// Draws a preview, with its own scrolling. It takes the room it's given and nothing more: whatever is in the
/// file, it can't push the panel it's in any wider, because that would run the text off the window.
pub fn draw(ui: &mut Ui, blocks: &[Block]) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, egui::Sense::hover());
    let mut contained = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Min)));
    contained.set_clip_rect(area.intersect(ui.clip_rect()));
    egui::ScrollArea::vertical().id_salt("preview-scroll").auto_shrink([false, false]).show(&mut contained, |ui| {
        ui.set_min_width(ui.available_width());
        if blocks.is_empty() {
            ui.label(RichText::new("There's nothing to preview yet.").color(theme::text_faint()));
        }
        for block in blocks {
            let shift = block.indent as f32 * 18.0 + block.quote as f32 * 12.0;
            if shift == 0.0 {
                draw_block(ui, block);
                continue;
            }
            let response = ui.horizontal_top(|ui| {
                ui.add_space(shift);
                ui.vertical(|ui| draw_block(ui, block));
            });
            for depth in 0..block.quote {
                let x = response.response.rect.min.x + 4.0 + depth as f32 * 12.0;
                ui.painter().vline(x, response.response.rect.y_range(), Stroke::new(2.0, theme::border()));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(block: &Block) -> String {
        block.spans.iter().map(|span| span.text.as_str()).collect()
    }

    #[test]
    fn markdown_files_are_recognised_by_name_or_language() {
        for name in ["a.md", "A.MD", "b.markdown", "c.mdown"] {
            assert_eq!(kind_for(Some(Path::new(name)), "No highlighting"), Some(Kind::Markdown), "{name}");
        }
        assert_eq!(kind_for(None, "Markdown"), Some(Kind::Markdown), "an unsaved file viewed as Markdown");
        assert_eq!(kind_for(Some(Path::new("a.txt")), "No highlighting"), None);
        assert_eq!(kind_for(Some(Path::new("a.html")), "HTML"), None, "not yet");
        assert_eq!(kind_for(None, "No highlighting"), None);
    }

    #[test]
    fn headings_and_paragraphs() {
        let blocks = render(Kind::Markdown, "# One\n\n## Two\n\nSome text\nwrapped here.\n\nNext.");
        assert_eq!(blocks[0].kind, BlockKind::Heading(1));
        assert_eq!(plain(&blocks[0]), "One");
        assert_eq!(blocks[1].kind, BlockKind::Heading(2));
        assert_eq!(plain(&blocks[2]), "Some text wrapped here.");
        assert_eq!(plain(&blocks[3]), "Next.");
    }

    #[test]
    fn inline_styles_carry_through_and_nest() {
        let blocks = render(Kind::Markdown, "a **b *c*** ~~d~~ `e`");
        let spans = &blocks[0].spans;
        let find = |text: &str| spans.iter().find(|span| span.text == text).unwrap();
        assert!(find("b ").strong && !find("b ").italic);
        assert!(find("c").strong && find("c").italic);
        assert!(find("d").strike);
        assert!(find("e").code);
        assert!(!find("a ").strong);
    }

    #[test]
    fn lists_have_markers_numbers_and_depth() {
        let blocks = render(Kind::Markdown, "- one\n  - inner\n- two\n\n3. three\n4. four\n");
        assert_eq!(blocks[0].marker.as_deref(), Some("\u{2022}"));
        assert_eq!(blocks[0].indent, 1);
        assert_eq!(plain(&blocks[1]), "inner");
        assert_eq!(blocks[1].indent, 2);
        assert_eq!(plain(&blocks[2]), "two");
        assert_eq!(blocks[2].indent, 1, "back out of the inner list");
        assert_eq!(blocks[3].marker.as_deref(), Some("3."), "numbered lists start where they say");
        assert_eq!(blocks[4].marker.as_deref(), Some("4."));
    }

    #[test]
    fn task_lists_show_their_boxes() {
        let blocks = render(Kind::Markdown, "- [x] done\n- [ ] not yet\n");
        assert!(plain(&blocks[0]).starts_with('\u{2611}'));
        assert!(plain(&blocks[1]).starts_with('\u{2610}'));
    }

    #[test]
    fn code_blocks_keep_their_lines_and_quotes_their_depth() {
        let blocks = render(Kind::Markdown, "```rust\nlet a = 1;\n  let b = 2;\n```\n\n> quoted\n>> deeper\n");
        assert_eq!(blocks[0].kind, BlockKind::Code);
        assert_eq!(plain(&blocks[0]), "let a = 1;\n  let b = 2;\n");
        assert_eq!(blocks[1].quote, 1);
        assert_eq!(blocks[2].quote, 2);
    }

    #[test]
    fn tables_keep_their_rows_and_cells() {
        let blocks = render(Kind::Markdown, "| A | B |\n|---|---|\n| 1 | **2** |\n");
        let BlockKind::Table(rows) = &blocks[0].kind else {
            panic!("a table");
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][1][0].text, "B");
        assert!(rows[1][1][0].strong);
    }

    #[test]
    fn only_safe_links_can_be_followed() {
        let blocks = render(Kind::Markdown, "[web](https://example.com) [mail](mailto:a@b.c) [bad](javascript:alert(1)) [file](file:///C:/x) [near](other.md)");
        let spans = &blocks[0].spans;
        let link = |text: &str| spans.iter().find(|span| span.text == text).unwrap().link.clone();
        assert_eq!(link("web").as_deref(), Some("https://example.com"));
        assert_eq!(link("mail").as_deref(), Some("mailto:a@b.c"));
        assert_eq!(link("bad"), None);
        assert_eq!(link("file"), None);
        assert_eq!(link("near"), None);
    }

    #[test]
    fn images_arent_fetched_and_web_markup_isnt_acted_on() {
        let blocks = render(Kind::Markdown, "![a picture](https://example.com/x.png)\n\n<script>alert(1)</script>\n");
        assert_eq!(plain(&blocks[0]), "[image: a picture]");
        assert!(blocks[0].spans.iter().all(|span| span.link.is_none()));
        assert!(plain(&blocks[1]).contains("<script>"), "shown as it's written");
        assert!(blocks[1].spans[0].code);
    }

    #[test]
    fn the_link_under_the_pointer_is_found_by_character() {
        let blocks = render(Kind::Markdown, "go [here](https://e.x) now");
        let spans = &blocks[0].spans;
        assert_eq!(link_at(spans, 0), None);
        assert_eq!(link_at(spans, 3), Some("https://e.x"));
        assert_eq!(link_at(spans, 6), Some("https://e.x"));
        assert_eq!(link_at(spans, 7), None);
        assert_eq!(link_at(spans, 500), None);
    }

    #[test]
    fn odd_or_empty_text_never_goes_wrong() {
        assert!(render(Kind::Markdown, "").is_empty());
        for text in ["*", "**", "[", "[a](", "```", "> ", "- ", "|", "<", "#", "\u{0}", "- [ ]", "1.", "~~", "![", "\r\n\r\n"] {
            let _ = render(Kind::Markdown, text);
        }
    }

    #[test]
    fn columns_that_fit_keep_their_widths() {
        assert_eq!(column_widths(&[50.0, 100.0], 500.0), [50.0, 100.0]);
        assert!(column_widths(&[], 500.0).is_empty());
    }

    #[test]
    fn when_too_wide_narrow_columns_stay_and_wide_ones_share_the_rest() {
        let widths = column_widths(&[40.0, 900.0, 900.0], 424.0);
        assert_eq!(widths[0], 40.0);
        assert_eq!(widths[1], 180.0);
        assert_eq!(widths[2], 180.0);
    }

    #[test]
    fn columns_always_fit_however_little_room_there_is() {
        for available in [1000.0, 300.0, 100.0, 40.0] {
            for natural in [vec![500.0; 3], vec![10.0, 2000.0], vec![30.0; 8], vec![1000.0]] {
                let widths = column_widths(&natural, available);
                let gaps = TABLE_GAP * (natural.len() - 1) as f32;
                let room = (available - gaps).max(natural.len() as f32);
                assert_eq!(widths.len(), natural.len());
                assert!(widths.iter().sum::<f32>() <= room + 0.01, "{natural:?} in {available}: {widths:?}");
                assert!(widths.iter().all(|width| *width > 0.0));
            }
        }
    }

    fn wide_blocks() -> Vec<Block> {
        let long = "plenty of words that go on for a long way past any sensible edge of the preview and keep on going ".repeat(4);
        let word = "x".repeat(600);
        let text = format!(
            "# {long}\n\n{long}\n\n{word}\n\n[{word}](https://e.x)\n\n- {long}\n  - {long}\n- [x] {word}\n\n1. {long}\n\n> {long}\n>> {word}\n\n```\n{long}\n{word}\n```\n\n| {word} | b | {long} |\n|---|---|---|\n| {long} | {word} | [{word}](https://e.x) |\n\n<div {word}>\n\n`{word}`\n"
        );
        render(Kind::Markdown, &text)
    }

    fn texts_in(shape: &egui::Shape, found: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Text(text) => found.push(text.visual_bounding_rect()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|one| texts_in(one, found)),
            _ => {}
        }
    }

    #[test]
    fn wide_content_never_widens_its_panel_or_leaves_it() {
        for width in [600.0, 300.0, 170.0] {
            let blocks = wide_blocks();
            let ctx = egui::Context::default();
            let mut panel = egui::Rect::NOTHING;
            let mut texts = Vec::new();
            // Several frames, because a panel takes on a size it was pushed to on the next one.
            for _ in 0..6 {
                let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), ..Default::default() };
                let mut output = ctx.run_ui(raw, |ui| {
                    panel = egui::Panel::right("preview-test").default_size(width).size_range(100.0..=1400.0).frame(egui::Frame::NONE).show(ui, |ui| draw(ui, &blocks)).response.rect;
                });
                output.textures_delta.clear();
                texts.clear();
                output.shapes.iter().for_each(|clipped| texts_in(&clipped.shape, &mut texts));
            }
            assert!((panel.width() - width).abs() < 1.0, "the panel stays at {width}, not {}", panel.width());
            assert!(texts.len() > 3, "the text was looked at");
            for text in &texts {
                assert!(text.min.x >= panel.min.x - 1.0 && text.max.x <= panel.max.x + 3.0, "text at {text:?} is outside the panel {panel:?}");
            }
        }
    }

    #[test]
    fn every_kind_of_block_draws_in_both_themes() {
        let text = "# H1\n\n## H2\n\n### H3\n\ntext **b** *i* `c` [l](https://e.x)\n\n- a\n  - b\n\n1. x\n\n> q\n\n```\ncode\n```\n\n---\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
        let blocks = render(Kind::Markdown, text);
        for light in [false, true] {
            theme::set_light(light);
            let ctx = egui::Context::default();
            for _ in 0..2 {
                let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))), ..Default::default() };
                let mut output = ctx.run_ui(raw, |ui| draw(ui, &blocks));
                output.textures_delta.clear();
            }
        }
        theme::set_light(false);
    }
}
