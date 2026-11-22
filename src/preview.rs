//! The preview beside the text. Markdown is the first format; another joins by adding a `Kind`, saying which
//! files it's for in `kind_for`, and turning its text into `Block`s in `render`.
//! Nothing here runs anything or fetches anything: links only open when clicked, and the only pictures shown are
//! files beside the one previewed (a Markdown guide's own pictures); nothing is fetched from the web.

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

/// The bold font, where there is one, for other places that show bold.
pub fn bold_family() -> Option<FontFamily> {
    HAS_BOLD.load(std::sync::atomic::Ordering::Relaxed).then(|| FontFamily::Name(BOLD_FAMILY.into()))
}

/// The most text a preview will lay out. It's redone as you type, so a huge file would make typing slow.
pub const MOST_PREVIEW_BYTES: usize = 4 * 1024 * 1024;
/// Music and sound can be larger: modules with many samples run to tens of megabytes, and sound files more.
pub const MOST_SOUND_BYTES: usize = 512 * 1024 * 1024;

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
    /// Music: a tracker module (ProTracker, Scream Tracker 3, FastTracker 2 or Impulse Tracker), a C64 SID tune,
    /// or Atari ST SNDH or YM music.
    Module,
    /// A sound file: MP3, WAV, FLAC, Ogg, AIFF, 8SVX, VOC or AU.
    Sound,
    /// An old word processor's document, converted and laid out.
    Document,
    /// A web page, laid out from its HTML: headings, paragraphs, lists, tables and links, with no scripts or styles.
    Html,
    /// An SVG drawing, drawn.
    Svg,
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
            Kind::Module => "Music module",
            Kind::Sound => "Sound",
            Kind::Document => "Converted document",
            Kind::Html => "Web page",
            Kind::Svg => "SVG drawing",
        }
    }

    /// Drawn as a machine's screen, rather than laid out as a document.
    pub fn is_screen(self) -> bool {
        !matches!(self, Kind::Markdown | Kind::Html | Kind::Basic | Kind::Picture | Kind::Module | Kind::Sound | Kind::Document | Kind::Svg)
    }

    /// Made from the file's exact bytes rather than its text, so it works from hex mode too.
    pub fn reads_bytes(self) -> bool {
        !matches!(self, Kind::Markdown | Kind::Html)
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
    if path.and_then(Path::file_name).is_some_and(|n| crate::protracker::named_as_module(&n.to_string_lossy()) || crate::sid::named_as_sid(&n.to_string_lossy())) && language != "Modula-2" {
        return Some(Kind::Module);
    }
    if path.and_then(Path::file_name).is_some_and(|n| crate::st_music::named_as_sndh(&n.to_string_lossy()) || crate::st_music::named_as_ym(&n.to_string_lossy()) || crate::tracker::named_as_pc_module(&n.to_string_lossy())) {
        return Some(Kind::Module);
    }
    if extension.as_deref().is_some_and(|e| crate::sound::ENDINGS.contains(&e) && e != "snd") {
        return Some(Kind::Sound);
    }
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
        Kind::AnsiArt | Kind::Markdown | Kind::Html | Kind::Basic | Kind::Picture | Kind::Module | Kind::Sound | Kind::Document | Kind::Svg => crate::ansi_art::render(bytes, dos),
    }
}

/// Whether text is shown as the Amiga's console shows it, in its pens.
pub fn is_amiga_console(format: &crate::codec::TextFormat) -> bool {
    format.display == crate::codec::Display::AmigaConsole || format.charset.id == "amiga-text"
}

/// Makes a preview from a file's bytes: a retro screen, or a BASIC program's listing.
/// The longest side an SVG is drawn at, and the shortest it's drawn up to, so an icon isn't a speck.
const SVG_MOST: f32 = 2048.0;
const SVG_LEAST: f32 = 256.0;

/// The fonts an SVG's text is drawn in: the computer's own, found once.
fn svg_fonts() -> std::sync::Arc<resvg::usvg::fontdb::Database> {
    static FONTS: std::sync::OnceLock<std::sync::Arc<resvg::usvg::fontdb::Database>> = std::sync::OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut fonts = resvg::usvg::fontdb::Database::new();
            fonts.load_system_fonts();
            std::sync::Arc::new(fonts)
        })
        .clone()
}

/// An SVG drawn as a picture, at its own size (made larger if it's tiny, smaller if it's huge), on a clear
/// background. Pictures it links to are looked for beside it.
pub fn svg_picture(bytes: &[u8], folder: Option<&Path>) -> Result<crate::pictures::Picture, String> {
    let options = resvg::usvg::Options { resources_dir: folder.map(Path::to_path_buf), fontdb: svg_fonts(), ..resvg::usvg::Options::default() };
    let tree = resvg::usvg::Tree::from_data(bytes, &options).map_err(|e| crate::i18n::tf!("This SVG can't be drawn: {e}"))?;
    let size = tree.size();
    let longest = size.width().max(size.height()).max(1.0);
    let scale = if longest > SVG_MOST { SVG_MOST / longest } else if longest < SVG_LEAST { SVG_LEAST / longest } else { 1.0 };
    let (width, height) = (((size.width() * scale).ceil() as u32).max(1), ((size.height() * scale).ceil() as u32).max(1));
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or_else(|| crate::i18n::tr("This SVG is too large to draw.").to_string())?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    // tiny-skia keeps colours multiplied by their opacity; a picture here has them as they are.
    let rgba: Vec<u8> = pixmap.pixels().iter().flat_map(|p| {
        let c = p.demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }).collect();
    let mut info = vec![(crate::i18n::tr("Size").to_string(), format!("{:.0} \u{D7} {:.0}", size.width(), size.height()))];
    if (scale - 1.0).abs() > f32::EPSILON {
        info.push((crate::i18n::tr("Drawn at").to_string(), format!("{width} \u{D7} {height}")));
    }
    Ok(crate::pictures::Picture { width: width as usize, height: height as usize, rgba, aspect: 1.0, info, cycling: None })
}

pub fn render_bytes(kind: Kind, bytes: &[u8], format: &crate::codec::TextFormat, path: Option<&Path>) -> FromBytes {
    if kind == Kind::Svg {
        let folder = path.and_then(Path::parent).map(Path::to_path_buf);
        return match svg_picture(bytes, folder.as_deref()) {
            Ok(picture) => FromBytes::Picture(picture),
            Err(why) => FromBytes::Nothing(why),
        };
    }
    if kind == Kind::Picture {
        let name = path.and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        return match crate::pictures::detect(&name, bytes).ok_or_else(|| crate::i18n::tr("This isn't a picture Limoniter can show any more.").to_string()).and_then(|f| crate::pictures::decode_named(f, &name, bytes)) {
            Ok(picture) => FromBytes::Picture(picture),
            Err(why) => FromBytes::Nothing(why),
        };
    }
    if kind == Kind::Document {
        let name = path.and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let converted = crate::legacy_docs::detect(&name, bytes).ok_or_else(|| crate::i18n::tr("This isn't a document Limoniter can convert any more.").to_string()).and_then(|kind| crate::legacy_docs::convert(kind, bytes));
        return match converted {
            Ok(doc) => {
                let mut markdown = crate::legacy_docs::to_markdown(&doc);
                markdown += &format!("\n\n---\n\n*Converted from {}.*\n", doc.format);
                for note in &doc.notes {
                    markdown += &format!("\n*{note}*\n");
                }
                FromBytes::Blocks(render(Kind::Markdown, &markdown))
            }
            Err(why) => FromBytes::Nothing(why),
        };
    }
    if kind == Kind::Module {
        let name = path.and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if crate::sid::looks_like(bytes) || crate::sid::named_as_sid(&name) {
            return match crate::sid::read(bytes) {
                Ok(tune) => FromBytes::Blocks(tune.blocks()),
                Err(why) => FromBytes::Nothing(why),
            };
        }
        // Atari ST music (SNDH or YM) goes by what it is, and anything else is taken for a ProTracker module.
        if crate::st_music::recognised(&bytes[..bytes.len().min(64)], &name) {
            return match crate::st_music::read(bytes, &name) {
                Ok(music) => FromBytes::Blocks(music.blocks()),
                Err(why) => FromBytes::Nothing(why),
            };
        }
        // Scream Tracker 3, FastTracker 2 and Impulse Tracker go by their marks.
        if crate::tracker::pc_format(bytes).is_some() {
            return match crate::tracker::load(bytes, &name) {
                Ok(song) => FromBytes::Blocks(song.blocks()),
                Err(why) => FromBytes::Nothing(why),
            };
        }
        return match crate::protracker::read(bytes, &name) {
            Ok(module) => FromBytes::Blocks(module.blocks()),
            Err(why) => FromBytes::Nothing(why),
        };
    }
    if kind == Kind::Sound {
        let name = path.and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        return match crate::sound::info(bytes, &name) {
            Ok(info) => FromBytes::Blocks(info.blocks()),
            Err(why) => FromBytes::Nothing(why),
        };
    }
    if kind != Kind::Basic {
        return FromBytes::Screen(render_screen(kind, bytes, format));
    }
    let extension = path.and_then(Path::extension).and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    if !crate::basic::is_program_file(&extension, Some(bytes)) {
        if !matches!(extension.as_str(), "bas" | "bbc") {
            return FromBytes::Nothing(crate::i18n::tf!("This .{extension} file isn't a BASIC program."));
        }
        return FromBytes::Nothing(crate::i18n::tr("This .bas file is a listing kept as text, not a tokenised program. Format, Tokenised BASIC can save it as one.").to_string());
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
        blocks.push(paragraph(vec![Span { text: crate::i18n::tr("The screen when it was saved").into(), strong: true, ..Span::default() }]));
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
        Some("html" | "htm" | "xhtml") => Some(Kind::Html),
        Some("svg" | "svgz") => Some(Kind::Svg),
        _ if language == "SVG" => Some(Kind::Svg),
        _ if language == "HTML" => Some(Kind::Html),
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
    /// A picture beside the file, by its path from the file's folder, and the words standing for it.
    Picture(String, String),
    /// A Mermaid diagram's text, drawn as the diagram.
    Diagram(String),
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
        Kind::Html => html(text),
        _ => Vec::new(),
    }
}

/// Lays out a web page from its HTML. The parts that are about how it looks or what it does (scripts, styles, the
/// head) are left out, and nothing in it is ever run or fetched: links are shown as links, pictures by what they
/// say they are. Tags that aren't closed, as browsers allow, close with the block around them.
fn html(text: &str) -> Vec<Block> {
    let mut b = Builder::default();
    // Inside a script, style or the head, whose text isn't shown, and how deep.
    let mut hidden: Option<(String, usize)> = None;
    let mut code = 0usize;
    let mut pre = 0usize;
    let mut heading: Option<u8> = None;
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |at| &after[at + 3..]);
            continue;
        }
        if rest.starts_with('<') && rest[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!' || c == '?') {
            let end = tag_end(rest);
            let tag = &rest[1..end.saturating_sub(1).max(1)];
            rest = &rest[end..];
            let closing = tag.starts_with('/');
            let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
            if let Some((hidden_name, depth)) = &mut hidden {
                if name == *hidden_name {
                    if closing {
                        *depth -= 1;
                        if *depth == 0 {
                            hidden = None;
                        }
                    } else {
                        *depth += 1;
                    }
                }
                continue;
            }
            if closing {
                html_close(&mut b, &name, &mut code, &mut pre, &mut heading);
            } else {
                if matches!(name.as_str(), "script" | "style" | "head" | "noscript" | "template" | "svg" | "object" | "iframe") && !tag.ends_with('/') {
                    hidden = Some((name, 1));
                    continue;
                }
                html_open(&mut b, &name, tag, &mut code, &mut pre, &mut heading);
            }
            continue;
        }
        let next = rest[1..].find('<').map_or(rest.len(), |at| at + 1);
        let words = decode_entities(&rest[..next]);
        rest = &rest[next..];
        if hidden.is_some() {
            continue;
        }
        if pre > 0 {
            b.push_text(&words, true);
            continue;
        }
        let mut collapsed = String::with_capacity(words.len());
        for c in words.chars() {
            if c.is_whitespace() && c != '\u{A0}' {
                if !collapsed.ends_with(' ') {
                    collapsed.push(' ');
                }
            } else {
                collapsed.push(c);
            }
        }
        // A space at the start of a block, or after one already there, isn't shown.
        let at_start = b.spans.last().is_none_or(|span| span.text.ends_with([' ', '\n']));
        let collapsed = if at_start { collapsed.trim_start() } else { collapsed.as_str() };
        if !collapsed.is_empty() {
            b.push_text(collapsed, code > 0);
        }
    }
    if let Some(level) = heading {
        b.push_block(BlockKind::Heading(level));
    }
    if pre > 0 {
        b.push_block(BlockKind::Code);
    }
    if let Some(rows) = b.table.take() {
        b.flush();
        b.push_block(BlockKind::Table(rows));
    }
    b.flush();
    b.blocks
}

/// Where a tag that starts the text ends, just past its `>`, passing over any `>` in quoted attributes.
fn tag_end(text: &str) -> usize {
    let mut quote = None;
    for (at, c) in text.char_indices().skip(1) {
        match (c, quote) {
            ('"' | '\'', None) => quote = Some(c),
            (c, Some(q)) if c == q => quote = None,
            ('>', None) => return at + 1,
            _ => {}
        }
    }
    text.len()
}

/// An attribute's value from inside a tag.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let at = from + found;
        from = at + name.len();
        let before_ok = lower[..at].ends_with(|c: char| c.is_whitespace());
        let after = lower[from..].trim_start();
        if !before_ok || !after.starts_with('=') {
            continue;
        }
        let value_start = tag.len() - after.len() + 1;
        let value = tag[value_start..].trim_start();
        let raw = match value.chars().next() {
            Some(q @ ('"' | '\'')) => value[1..].split(q).next().unwrap_or(""),
            _ => value.split(|c: char| c.is_whitespace() || c == '>' || c == '/').next().unwrap_or(""),
        };
        return Some(decode_entities(raw));
    }
    None
}

/// The end of a block of text, unless it's inside a table's cell, where it all stays in the cell.
fn block_break(b: &mut Builder) {
    if b.table.is_none() {
        b.flush();
    }
}

fn html_open(b: &mut Builder, name: &str, tag: &str, code: &mut usize, pre: &mut usize, heading: &mut Option<u8>) {
    match name {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            b.flush();
            *heading = Some(name.as_bytes()[1] - b'0');
        }
        "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "nav" | "aside" | "figure" | "figcaption" | "address" | "dl" | "dt" | "dd" | "form" | "fieldset" | "details" | "summary" | "center" | "body" | "caption" => block_break(b),
        "br" => b.push_text("\n", false),
        "hr" => {
            b.flush();
            b.push_block(BlockKind::Rule);
        }
        "b" | "strong" => b.strong += 1,
        "i" | "em" | "cite" | "var" | "dfn" => b.italic += 1,
        "s" | "strike" | "del" => b.strike += 1,
        "code" | "kbd" | "samp" | "tt" => *code += 1,
        "a" => b.link = attribute(tag, "href").and_then(|href| safe_link(&href)),
        "img" => {
            let alt = attribute(tag, "alt").filter(|alt| !alt.trim().is_empty()).unwrap_or_else(|| "picture".into());
            b.push_text(&format!("[image: {}]", alt.trim()), false);
        }
        "ul" | "ol" | "menu" => {
            b.flush();
            let start = attribute(tag, "start").and_then(|s| s.trim().parse().ok()).unwrap_or(1);
            b.lists.push((name == "ol").then_some(start));
        }
        "li" => {
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
        "blockquote" => {
            b.flush();
            b.quote += 1;
        }
        "pre" => {
            b.flush();
            *pre += 1;
        }
        "table" => {
            b.flush();
            b.table = Some(Vec::new());
        }
        "tr" => {
            b.flush_row();
            b.row = Vec::new();
        }
        "td" | "th" => b.flush_cell(),
        _ => {}
    }
}

fn html_close(b: &mut Builder, name: &str, code: &mut usize, pre: &mut usize, heading: &mut Option<u8>) {
    match name {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            if let Some(level) = heading.take() {
                b.push_block(BlockKind::Heading(level));
            }
        }
        "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "nav" | "aside" | "figure" | "figcaption" | "address" | "dl" | "dt" | "dd" | "form" | "fieldset" | "details" | "summary" | "center" | "li" | "caption" => block_break(b),
        "b" | "strong" => b.strong = b.strong.saturating_sub(1),
        "i" | "em" | "cite" | "var" | "dfn" => b.italic = b.italic.saturating_sub(1),
        "s" | "strike" | "del" => b.strike = b.strike.saturating_sub(1),
        "code" | "kbd" | "samp" | "tt" => *code = code.saturating_sub(1),
        "a" => b.link = None,
        "ul" | "ol" | "menu" => {
            b.flush();
            b.lists.pop();
        }
        "blockquote" => {
            b.flush();
            b.quote = b.quote.saturating_sub(1);
        }
        "pre" if *pre > 0 => {
            *pre -= 1;
            // The line end just after <pre> isn't part of what's shown.
            if let Some(first) = b.spans.first_mut()
                && let Some(rest) = first.text.strip_prefix('\n')
            {
                first.text = rest.to_string();
            }
            b.push_block(BlockKind::Code);
        }
        // A cell is kept even when it's empty, so the columns stay in line.
        "td" | "th" => {
            let cell = std::mem::take(&mut b.spans);
            if b.table.is_some() {
                b.row.push(cell);
            }
        }
        "tr" => b.flush_row(),
        "table" => {
            b.flush_row();
            if let Some(rows) = b.table.take() {
                b.push_block(BlockKind::Table(rows));
            }
        }
        _ => {}
    }
}

/// HTML's character references: the common named ones, and any by number.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let end = after.find(';').filter(|end| *end <= 10);
        let named = end.map(|end| &after[..end]);
        let found = named.and_then(|name| {
            if let Some(number) = name.strip_prefix('#') {
                let value = match number.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => number.parse().ok(),
                };
                return value.and_then(char::from_u32);
            }
            Some(match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => '\u{A0}',
                "copy" => '\u{A9}',
                "reg" => '\u{AE}',
                "trade" => '\u{2122}',
                "pound" => '\u{A3}',
                "euro" => '\u{20AC}',
                "deg" => '\u{B0}',
                "middot" => '\u{B7}',
                "bull" => '\u{2022}',
                "hellip" => '\u{2026}',
                "ndash" => '\u{2013}',
                "mdash" => '\u{2014}',
                "lsquo" => '\u{2018}',
                "rsquo" => '\u{2019}',
                "ldquo" => '\u{201C}',
                "rdquo" => '\u{201D}',
                "times" => '\u{D7}',
                "eacute" => '\u{E9}',
                "egrave" => '\u{E8}',
                "aacute" => '\u{E1}',
                "agrave" => '\u{E0}',
                "uuml" => '\u{FC}',
                "ouml" => '\u{F6}',
                "auml" => '\u{E4}',
                "szlig" => '\u{DF}',
                "ccedil" => '\u{E7}',
                _ => return None,
            })
        });
        match (found, end) {
            (Some(c), Some(end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            _ => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Only these are handed to the browser. Anything else, such as a script or a file on this computer, stays plain text.
fn safe_link(address: &str) -> Option<String> {
    let lower = address.trim().to_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")).then(|| address.trim().to_string())
}

/// What a link written in Markdown is followed as: a web address, or a file beside the one previewed (by its path
/// from that file's folder, marked "local:"), which Limoniter opens itself. A file further up than the folder, or by
/// a whole path, stays plain text.
fn markdown_link(address: &str) -> Option<String> {
    let address = address.trim();
    // A heading in the same file, as a table of contents links to them.
    if address.starts_with('#') && address.len() > 1 && !address.contains(char::is_whitespace) {
        return Some(address.to_string());
    }
    safe_link(address).or_else(|| local_path(address).map(|path| format!("{LOCAL}{path}")))
}

/// How a link to a file beside the one previewed is marked.
pub const LOCAL: &str = "local:";

/// A path from the previewed file's folder, if it's a plain one: no scheme, not from the top, nothing above.
fn local_path(address: &str) -> Option<String> {
    let address = address.trim().split('#').next()?.trim();
    let plain = !address.is_empty() && !address.contains(':') && !address.starts_with(['/', '\\']) && !address.split(['/', '\\']).any(|part| part == "..");
    plain.then(|| address.to_string())
}

#[derive(Default)]
struct Builder {
    blocks: Vec<Block>,
    /// A picture being read, and its words so far.
    picture: Option<(String, String)>,
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
    /// Inside a fenced block marked mermaid.
    in_mermaid: bool,
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

    /// Ends a table cell in HTML, where cells and rows needn't be closed.
    fn flush_cell(&mut self) {
        if self.table.is_some() && !self.spans.is_empty() {
            let cell = std::mem::take(&mut self.spans);
            self.row.push(cell);
        }
    }

    fn flush_row(&mut self) {
        self.flush_cell();
        if !self.row.is_empty()
            && let Some(table) = &mut self.table
        {
            table.push(std::mem::take(&mut self.row));
        }
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
                Tag::Link { dest_url, .. } => b.link = markdown_link(&dest_url),
                // A picture beside the file is shown; any other is only named.
                Tag::Image { dest_url, .. } => match local_path(&dest_url).filter(|p| is_picture_file(p)) {
                    Some(path) => {
                        b.flush();
                        b.picture = Some((path, String::new()));
                    }
                    None => b.push_text("[image: ", false),
                },
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
                Tag::CodeBlock(kind) => {
                    b.flush();
                    b.in_code = true;
                    b.in_mermaid = matches!(&kind, pulldown_cmark::CodeBlockKind::Fenced(info) if info.split_whitespace().next().is_some_and(|l| l.eq_ignore_ascii_case("mermaid")));
                }
                Tag::Table(_) => {
                    b.flush();
                    b.table = Some(Vec::new());
                }
                Tag::TableHead | Tag::TableRow => b.row = Vec::new(),
                Tag::HtmlBlock => b.flush(),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Strong => b.strong = b.strong.saturating_sub(1),
                TagEnd::Emphasis => b.italic = b.italic.saturating_sub(1),
                TagEnd::Strikethrough => b.strike = b.strike.saturating_sub(1),
                TagEnd::Link => b.link = None,
                TagEnd::Image => match b.picture.take() {
                    Some((path, alt)) => b.blocks.push(Block { kind: BlockKind::Picture(path, alt), spans: Vec::new(), indent: 0, quote: 0, marker: None }),
                    None => b.push_text("]", false),
                },
                TagEnd::Paragraph | TagEnd::Item | TagEnd::HtmlBlock => b.flush(),
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
                    if std::mem::take(&mut b.in_mermaid) {
                        let source: String = b.spans.drain(..).map(|span| span.text).collect();
                        b.push_block(BlockKind::Diagram(source));
                    } else {
                        b.push_block(BlockKind::Code);
                    }
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
                if let Some((_, alt)) = b.picture.as_mut() {
                    alt.push_str(&text);
                    continue;
                }
                let in_code = b.in_code;
                b.push_text(&text, in_code);
            }
            Event::Code(text) => b.push_text(&text, true),
            // A comment, such as a table of contents' markers, isn't shown, as it isn't on a web page.
            Event::Html(text) | Event::InlineHtml(text) if text.trim().starts_with("<!--") && text.trim().ends_with("-->") => {}
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
                // A file beside this one is opened by the window, a heading in this one scrolled to, and anything
                // else goes to the browser.
                if let Some(anchor) = address.strip_prefix('#') {
                    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(SCROLL_TO), anchor.to_lowercase()));
                } else {
                    match address.strip_prefix(LOCAL) {
                        Some(path) => {
                            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(OPEN_LOCAL), path.to_string()));
                        }
                        None => ui.ctx().open_url(egui::OpenUrl::new_tab(address)),
                    }
                }
            }
        }
    }
}

pub const TABLE_GAP: f32 = 12.0;

/// How wide each column of a table can be. If they all fit they keep their natural widths. If not, narrow
/// columns still keep theirs and the wide ones share what's left equally, wrapping their text to fit.
/// The result always fits, however little room there is.
pub fn column_widths(natural: &[f32], available: f32) -> Vec<f32> {
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
        BlockKind::Picture(_, alt) => {
            ui.label(RichText::new(format!("[image: {alt}]")).color(theme::text_faint()));
        }
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
        BlockKind::Diagram(source) => {
            draw_diagram(ui, source);
            ui.add_space(6.0);
        }
    }
}

/// A Mermaid diagram, as large as it was laid out or smaller to fit; or, if it can't be drawn, why, and its text.
fn draw_diagram(ui: &mut Ui, source: &str) {
    match &*crate::mermaid::cached(source) {
        Ok(scene) => {
            // Centred, as Mermaid's own are.
            let width = ui.available_width();
            let scale = (width / scene.width).min(1.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, scene.height * scale), egui::Sense::hover());
            if ui.is_rect_visible(rect) {
                let left = rect.min.x + (width - scene.width * scale) / 2.0;
                crate::diagram::paint(ui.painter(), scene, egui::pos2(left, rect.min.y), scale);
            }
        }
        Err(why) => {
            ui.label(RichText::new(why).color(theme::text_faint()).size(12.5));
            egui::Frame::new().fill(theme::ruler_back()).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(source.trim_end_matches('\n')).monospace().size(13.0));
            });
        }
    }
}

/// Draws a preview, with its own scrolling. It takes the room it's given and nothing more: whatever is in the
/// file, it can't push the panel it's in any wider, because that would run the text off the window.
/// Whether a path names a picture that can be shown beside a guide's text.
pub fn is_picture_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".png") || lower.ends_with(".svg")
}

/// A picture beside the file, read from its PNG or SVG.
pub fn read_local_picture(path: &Path) -> Option<crate::pictures::Picture> {
    let bytes = std::fs::read(path).ok()?;
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg")) {
        return svg_picture(&bytes, path.parent()).ok();
    }
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let (width, height) = (info.width as usize, info.height as usize);
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buffer[..width * height * 4].to_vec(),
        png::ColorType::GrayscaleAlpha => buffer[..width * height * 2].chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        _ => return None,
    };
    Some(crate::pictures::Picture { width, height, rgba, aspect: 1.0, info: Vec::new(), cycling: None })
}

/// The temporary data a click on a link to a file beside the previewed one leaves, for the window to open it.
pub const OPEN_LOCAL: &str = "preview-open-local";
/// The heading a click on a link to one leaves, for the preview to scroll to.
const SCROLL_TO: &str = "preview-scroll-to";

/// Each block's link name if it's a heading, as GitHub makes them (and the table of contents links to them): a
/// second heading with the same words gets -1 on the end, a third -2.
pub fn heading_anchors(blocks: &[Block]) -> Vec<Option<String>> {
    let mut used: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    blocks
        .iter()
        .map(|block| {
            let BlockKind::Heading(_) = block.kind else { return None };
            let text: String = block.spans.iter().map(|s| s.text.as_str()).collect();
            let base = crate::markdown_tools::anchor(text.trim());
            let count = used.entry(base.clone()).or_insert(0);
            let name = if *count == 0 { base } else { format!("{base}-{count}") };
            *count += 1;
            Some(name)
        })
        .collect()
}

/// A picture beside the file, read once and kept as a texture.
fn local_picture(ui: &Ui, path: &Path) -> Option<egui::TextureHandle> {
    let id = egui::Id::unique("preview-picture").with(path.to_path_buf());
    if let Some(texture) = ui.ctx().data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(texture);
    }
    let picture = read_local_picture(path)?;
    let texture = ui.ctx().load_texture(path.display().to_string(), egui::ColorImage::from_rgba_unmultiplied([picture.width, picture.height], &picture.rgba), egui::TextureOptions::LINEAR);
    ui.ctx().data_mut(|d| d.insert_temp(id, texture.clone()));
    Some(texture)
}

/// Draws the blocks; `folder` is the previewed file's own, where pictures and linked files beside it are found.
pub fn draw_in(ui: &mut Ui, blocks: &[Block], folder: Option<&Path>) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, egui::Sense::hover());
    let mut contained = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Min)));
    contained.set_clip_rect(area.intersect(ui.clip_rect()));
    egui::ScrollArea::vertical().id_salt("preview-scroll").auto_shrink([false, false]).show(&mut contained, |ui| {
        ui.set_min_width(ui.available_width());
        if blocks.is_empty() {
            ui.label(RichText::new(crate::i18n::tr("There's nothing to preview yet.")).color(theme::text_faint()));
        }
        let wanted: Option<String> = ui.ctx().data(|d| d.get_temp(egui::Id::unique(SCROLL_TO)));
        let anchors = if wanted.is_some() { heading_anchors(blocks) } else { Vec::new() };
        for (number, block) in blocks.iter().enumerate() {
            if let Some(wanted) = &wanted
                && anchors.get(number).is_some_and(|a| a.as_ref() == Some(wanted))
            {
                let top = ui.cursor().min;
                ui.scroll_to_rect(egui::Rect::from_min_size(top, egui::vec2(1.0, 24.0)), Some(egui::Align::TOP));
                ui.ctx().data_mut(|d| d.remove::<String>(egui::Id::unique(SCROLL_TO)));
            }
            if let BlockKind::Picture(path, alt) = &block.kind {
                match folder.and_then(|f| local_picture(ui, &f.join(path))) {
                    Some(texture) => {
                        // At its own size, or smaller to fit.
                        let size = texture.size_vec2();
                        let scale = (ui.available_width() / size.x).min(1.0);
                        ui.add_space(4.0);
                        ui.add(egui::Image::new((texture.id(), size * scale))).on_hover_text(alt.clone());
                        ui.add_space(4.0);
                    }
                    None => {
                        ui.label(RichText::new(format!("[image: {alt}]")).color(theme::text_faint()));
                    }
                }
                continue;
            }
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

/// Draws blocks where the ui is, with no scroll area of their own, as a notebook shows a Markdown cell.
pub fn draw_inline(ui: &mut Ui, blocks: &[Block]) {
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
}

#[cfg(test)]
mod guide_tests {
    use super::*;

    #[test]
    fn a_guides_own_links_and_pictures_are_followed_and_nothing_else() {
        let blocks = markdown("See [the copper](06-copper.md#waits), [the web](https://example.org), [up](../secret.md) and [all](/etc/passwd).\n\n![A demo](images/ch04.png)\n\n![Far away](https://example.org/x.png)\n");
        let links: Vec<Option<&str>> = blocks[0].spans.iter().filter(|s| !s.text.trim().is_empty() && !s.text.contains(',') && s.text != " and ").map(|s| s.link.as_deref()).collect();
        assert!(links.contains(&Some("local:06-copper.md")), "{links:?}");
        assert!(links.contains(&Some("https://example.org")));
        let plain: Vec<&str> = blocks[0].spans.iter().filter(|s| s.link.is_none()).map(|s| s.text.as_str()).collect();
        assert!(plain.iter().any(|t| t.contains("up")) && plain.iter().any(|t| t.contains("all")), "a file above, or by its whole path, isn't a link: {plain:?}");
        assert_eq!(blocks[1].kind, BlockKind::Picture("images/ch04.png".into(), "A demo".into()));
        assert!(blocks.iter().all(|b| !matches!(&b.kind, BlockKind::Picture(p, _) if p.contains("example.org"))), "nothing from the web is fetched");
    }
}

#[cfg(test)]
mod svg_tests {
    use super::*;

    #[test]
    fn svg_is_drawn_as_a_picture() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300"><rect x="0" y="0" width="200" height="300" fill="#ff0000"/></svg>"##;
        let picture = svg_picture(svg, None).unwrap();
        assert_eq!((picture.width, picture.height), (400, 300));
        assert_eq!(&picture.rgba[..4], &[255, 0, 0, 255], "red at the left");
        assert_eq!(picture.rgba[(300 * 400) * 4 - 1], 0, "clear at the right");
        // A small icon is drawn larger.
        let icon = svg_picture(br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"/>"#, None).unwrap();
        assert_eq!(icon.width, 256);
        assert!(svg_picture(b"<svg", None).is_err());
        assert_eq!(kind_for(Some(Path::new("logo.svg")), ""), Some(Kind::Svg));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(block: &Block) -> String {
        block.spans.iter().map(|span| span.text.as_str()).collect()
    }

    fn texts(blocks: &[Block]) -> Vec<String> {
        blocks.iter().map(|b| b.spans.iter().map(|s| s.text.as_str()).collect::<String>()).collect()
    }

    #[test]
    fn a_web_page_is_laid_out_without_its_scripts_or_styles() {
        let page = "<!DOCTYPE html><html><head><title>T</title><style>p{}</style></head><body>\n<h1>Big &amp; bold</h1>\n<p>One <b>two</b>\n   <i>three</i> <a href=\"https://example.com/x?a=1&amp;b=2\">four</a> <a href=\"javascript:alert(1)\">five</a></p>\n<script>document.write('<p>no</p>')</script>\n<ul><li>a<li>b</ul><ol start=\"3\"><li>c</li></ol>\n<pre>\n  keep   this\n</pre><hr><img src=x.png alt=\"A cat\"><br>caf&eacute; &#169; &#x263A;\n<!-- not shown <p>hidden</p> -->\n<table><tr><th>N</th><th>V</th></tr><tr><td>x</td><td></td></tr></table></body></html>";
        let blocks = html(page);
        let shown = texts(&blocks);
        assert_eq!(blocks[0].kind, BlockKind::Heading(1));
        assert_eq!(shown[0], "Big & bold");
        assert_eq!(shown[1], "One two three four five");
        assert!(blocks[1].spans.iter().any(|s| s.strong && s.text == "two") && blocks[1].spans.iter().any(|s| s.italic && s.text == "three"));
        assert_eq!(blocks[1].spans.iter().find(|s| s.text == "four").and_then(|s| s.link.clone()).as_deref(), Some("https://example.com/x?a=1&b=2"));
        assert!(blocks[1].spans.iter().find(|s| s.text == "five").is_some_and(|s| s.link.is_none()), "a script link isn't a link");
        assert!(!shown.iter().any(|t| t.contains("no") && t.contains('p') || t.contains("hidden") || t.contains("p{}")));
        assert_eq!(blocks.iter().filter_map(|b| b.marker.clone()).collect::<Vec<_>>(), ["\u{2022}", "\u{2022}", "3."]);
        let code = blocks.iter().find(|b| b.kind == BlockKind::Code).unwrap();
        assert_eq!(texts(std::slice::from_ref(code))[0], "  keep   this\n");
        assert!(blocks.iter().any(|b| b.kind == BlockKind::Rule));
        assert!(shown.iter().any(|t| t.trim_end() == "[image: A cat]\ncaf\u{E9} \u{A9} \u{263A}"), "{shown:?}");
        let Some(BlockKind::Table(rows)) = blocks.iter().map(|b| b.kind.clone()).find(|k| matches!(k, BlockKind::Table(_))) else { panic!("no table") };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].len(), 2, "the empty cell is kept");
        assert_eq!(kind_for(Some(Path::new("index.HTM")), ""), Some(Kind::Html));
    }

    #[test]
    fn markdown_files_are_recognised_by_name_or_language() {
        for name in ["a.md", "A.MD", "b.markdown", "c.mdown"] {
            assert_eq!(kind_for(Some(Path::new(name)), "No highlighting"), Some(Kind::Markdown), "{name}");
        }
        assert_eq!(kind_for(None, "Markdown"), Some(Kind::Markdown), "an unsaved file viewed as Markdown");
        assert_eq!(kind_for(Some(Path::new("a.txt")), "No highlighting"), None);
        assert_eq!(kind_for(Some(Path::new("a.html")), "HTML"), Some(Kind::Html), "a web page has its own preview");
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
        // A file beside this one is opened by Limoniter itself, not handed to anything else.
        assert_eq!(link("near").as_deref(), Some("local:other.md"));
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
                    panel = egui::Panel::right("preview-test").default_size(width).size_range(100.0..=1400.0).frame(egui::Frame::NONE).show(ui, |ui| draw_in(ui, &blocks, None)).response.rect;
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
                let mut output = ctx.run_ui(raw, |ui| draw_in(ui, &blocks, None));
                output.textures_delta.clear();
            }
        }
        theme::set_light(false);
    }
}
