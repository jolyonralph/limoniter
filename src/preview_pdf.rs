//! The preview saved as a PDF, laid out for A4 paper: headings, paragraphs with their bold, italic and code,
//! lists, quotes, tables, code, pictures and Mermaid diagrams, over as many pages as it takes, each numbered. The
//! headings become the PDF's bookmarks, and links to them (a table of contents' links) go to their pages.

use crate::pdf::{Bookmark, Document, Font, Page, Paint, Rgb, Target};
use crate::preview::{Block, BlockKind, Span};
use std::collections::HashMap;
use std::path::Path;

const LEFT: f32 = 56.0;
const RIGHT: f32 = 56.0;
const TOP: f32 = 56.0;
/// Where the text stops, above the page number.
const BOTTOM_MARGIN: f32 = 64.0;
const BODY: f32 = 10.5;
const CODE: f32 = 9.0;
const LEADING: f32 = 1.4;

fn ink() -> Rgb {
    Rgb::from_bytes(0x1E, 0x1E, 0x1E)
}

fn faint() -> Rgb {
    Rgb::from_bytes(0x70, 0x70, 0x70)
}

fn link_ink() -> Rgb {
    Rgb::from_bytes(0x1A, 0x4D, 0xB3)
}

fn shade() -> Rgb {
    Rgb::from_bytes(0xF2, 0xF2, 0xF2)
}

fn rule() -> Rgb {
    Rgb::from_bytes(0xC8, 0xC8, 0xC8)
}

fn heading_size(level: u8) -> f32 {
    match level {
        1 => 20.0,
        2 => 16.0,
        3 => 13.5,
        4 => 12.0,
        _ => 11.0,
    }
}

/// A word (or a run of code) ready to place: its text, how it's drawn, and its width.
#[derive(Clone, Debug)]
struct Piece {
    text: String,
    font: Font,
    size: f32,
    link: Option<String>,
    code: bool,
    strike: bool,
    width: f32,
    /// Whether a space comes before it, and how wide.
    space: f32,
}

/// A line of pieces, each at its place from the line's start.
type Line = Vec<(f32, Piece)>;

/// Words broken into lines no wider than `width`. A word wider than a line is broken where it has to be.
fn lines_of(spans: &[Span], size: f32, bold_all: bool, width: f32) -> Vec<Line> {
    let mut pieces: Vec<Option<Piece>> = Vec::new();
    for span in spans {
        let font = Font::styled(span.strong || bold_all, span.italic, span.code);
        let size = if span.code { size - 1.0 } else { size };
        let space = crate::pdf::text_width(" ", font, size);
        let mut pending_space = 0.0;
        for (n, part) in span.text.split('\n').enumerate() {
            if n > 0 {
                // A hard line break.
                pieces.push(None);
                pending_space = 0.0;
            }
            let mut word = String::new();
            let flush = |word: &mut String, pending: &mut f32, pieces: &mut Vec<Option<Piece>>| {
                if !word.is_empty() {
                    let text = std::mem::take(word);
                    let measured = crate::pdf::text_width(&text, font, size);
                    pieces.push(Some(Piece { text, font, size, link: span.link.clone(), code: span.code, strike: span.strike, width: measured, space: *pending }));
                    *pending = 0.0;
                }
            };
            for c in part.chars() {
                if c == ' ' || c == '\t' {
                    flush(&mut word, &mut pending_space, &mut pieces);
                    pending_space = space;
                } else {
                    word.push(c);
                }
            }
            flush(&mut word, &mut pending_space, &mut pieces);
        }
        // A space at the end of a span belongs before the next one's first word.
        if span.text.ends_with([' ', '\t']) {
            pieces.push(Some(Piece { text: String::new(), font, size, link: None, code: false, strike: false, width: 0.0, space }));
        }
    }
    let mut lines: Vec<Line> = vec![Vec::new()];
    let mut x = 0.0;
    let mut carried_space = 0.0;
    for piece in pieces {
        let Some(mut piece) = piece else {
            lines.push(Vec::new());
            x = 0.0;
            carried_space = 0.0;
            continue;
        };
        if piece.text.is_empty() {
            carried_space = piece.space;
            continue;
        }
        let space = if lines.last().is_some_and(Vec::is_empty) { 0.0 } else { piece.space.max(carried_space) };
        carried_space = 0.0;
        if x + space + piece.width <= width || lines.last().is_some_and(Vec::is_empty) && piece.width <= width {
            let at = x + space;
            x = at + piece.width;
            if let Some(line) = lines.last_mut() {
                line.push((at, piece));
            }
            continue;
        }
        if piece.width <= width {
            lines.push(Vec::new());
            x = piece.width;
            if let Some(line) = lines.last_mut() {
                line.push((0.0, piece));
            }
            continue;
        }
        // Too wide for any line: broken into lengths that fit.
        if !lines.last().is_some_and(Vec::is_empty) {
            lines.push(Vec::new());
        }
        let mut rest: Vec<char> = piece.text.chars().collect();
        while !rest.is_empty() {
            let mut take = rest.len();
            while take > 1 && crate::pdf::text_width(&rest[..take].iter().collect::<String>(), piece.font, piece.size) > width {
                take -= 1;
            }
            let text: String = rest.drain(..take).collect();
            piece.width = crate::pdf::text_width(&text, piece.font, piece.size);
            piece.text = text;
            x = piece.width;
            if let Some(line) = lines.last_mut() {
                line.push((0.0, piece.clone()));
            }
            if !rest.is_empty() {
                lines.push(Vec::new());
            }
        }
    }
    lines
}

struct Writer<'a> {
    document: Document,
    page: Page,
    y: f32,
    folder: Option<&'a Path>,
    /// Links to headings, to be pointed at their pages when every page is laid out: page, place, heading's name.
    heading_links: Vec<(usize, [f32; 4], String)>,
    anchors: HashMap<String, (usize, f32)>,
}

impl Writer<'_> {
    fn width(&self) -> f32 {
        self.document.width - LEFT - RIGHT
    }

    fn bottom(&self) -> f32 {
        self.document.height - BOTTOM_MARGIN
    }

    fn page_number(&self) -> usize {
        self.document.pages.len()
    }

    fn new_page(&mut self) {
        let page = std::mem::take(&mut self.page);
        self.document.pages.push(page);
        self.y = TOP;
    }

    /// Starts a new page unless there's this much room left on this one (and this one has something on it).
    fn room_for(&mut self, height: f32) {
        if self.y + height > self.bottom() && self.y > TOP + 0.5 {
            self.new_page();
        }
    }

    /// Lines of words, from `left`, each moved to a new page if it won't fit on this one.
    fn words(&mut self, spans: &[Span], left: f32, width: f32, size: f32, bold_all: bool, colour: Rgb) {
        let height = size * LEADING;
        for line in lines_of(spans, size, bold_all, width) {
            self.room_for(height);
            let baseline = self.y + size * 0.95;
            for (x, piece) in &line {
                let x = left + x;
                if piece.code {
                    self.page.rect(&self.document, x - 1.5, self.y + 1.0, piece.width + 3.0, height - 1.0, 2.0, Paint::fill(shade()));
                }
                let colour = if piece.link.is_some() { link_ink() } else { colour };
                self.page.text(&self.document, x, baseline, piece.font, piece.size, colour, &piece.text);
                if piece.strike {
                    self.page.path(&self.document, &[(x, baseline - piece.size * 0.3), (x + piece.width, baseline - piece.size * 0.3)], false, Paint::stroke(colour, 0.6));
                }
                if let Some(link) = &piece.link {
                    self.page.path(&self.document, &[(x, baseline + 1.2), (x + piece.width, baseline + 1.2)], false, Paint::stroke(link_ink(), 0.5));
                    let rect = [x, self.y, piece.width, height];
                    if let Some(anchor) = link.strip_prefix('#') {
                        self.heading_links.push((self.page_number(), rect, anchor.to_lowercase()));
                    } else if !link.starts_with(crate::preview::LOCAL) {
                        self.page.link(rect[0], rect[1], rect[2], rect[3], Target::Web(link.clone()));
                    }
                }
            }
            self.y += height;
        }
    }

    fn code(&mut self, text: &str, left: f32, width: f32) {
        let height = CODE * 1.3;
        let per_line = ((width - 12.0) / (CODE * 0.6)).floor().max(8.0) as usize;
        // A short block is kept on one page; a long one goes over as it must.
        let rows: usize = text.trim_end_matches('\n').split('\n').map(|l| l.chars().count().div_ceil(per_line).max(1)).sum();
        let whole = rows as f32 * height + 10.0;
        self.room_for(if whole < (self.bottom() - TOP) / 3.0 { whole } else { height + 8.0 });
        // A little shading above and below, then a shaded band a line at a time, so it can go over a page.
        self.page.rect(&self.document, left, self.y, width, 5.0, 0.0, Paint::fill(shade()));
        self.y += 5.0;
        for line in text.trim_end_matches('\n').split('\n') {
            let chars: Vec<char> = line.replace('\t', "    ").chars().collect();
            let pieces: Vec<String> = if chars.is_empty() { vec![String::new()] } else { chars.chunks(per_line).map(|c| c.iter().collect()).collect() };
            for piece in pieces {
                if self.y + height > self.bottom() {
                    self.new_page();
                }
                self.page.rect(&self.document, left, self.y, width, height + 0.3, 0.0, Paint::fill(shade()));
                self.page.text(&self.document, left + 6.0, self.y + CODE * 1.0, Font::Mono, CODE, ink(), &piece);
                self.y += height;
            }
        }
        self.page.rect(&self.document, left, self.y, width, 5.0, 0.0, Paint::fill(shade()));
        self.y += 5.0 + 6.0;
    }

    fn table(&mut self, rows: &[Vec<Vec<Span>>], left: f32, width: f32) {
        const SIZE: f32 = 9.5;
        const PAD: f32 = 3.0;
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return;
        }
        let rows: Vec<Vec<Vec<Span>>> = rows
            .iter()
            .enumerate()
            .map(|(n, row)| {
                let mut row = row.clone();
                if n == 0 {
                    row.iter_mut().flatten().for_each(|s| s.strong = true);
                }
                row
            })
            .collect();
        let mut natural = vec![0.0_f32; columns];
        for row in &rows {
            for (cell, w) in row.iter().zip(&mut natural) {
                let lines = lines_of(cell, SIZE, false, f32::INFINITY);
                let widest = lines.iter().map(|l| l.last().map_or(0.0, |(x, p)| x + p.width)).fold(0.0, f32::max);
                *w = w.max(widest.ceil() + 1.0);
            }
        }
        let widths = crate::preview::column_widths(&natural, width);
        for (n, row) in rows.iter().enumerate() {
            let laid: Vec<Vec<Line>> = row.iter().zip(&widths).map(|(cell, w)| lines_of(cell, SIZE, false, *w)).collect();
            let tallest = laid.iter().map(|l| l.len()).max().unwrap_or(1).max(1) as f32 * SIZE * LEADING + PAD * 2.0;
            self.room_for(tallest);
            if n % 2 == 1 {
                self.page.rect(&self.document, left - 3.0, self.y, width + 6.0, tallest, 0.0, Paint::fill(shade()));
            }
            let top = self.y;
            let mut x = left;
            for (cell, w) in row.iter().zip(&widths) {
                self.y = top + PAD;
                self.words(cell, x, *w, SIZE, false, ink());
                x += w + crate::preview::TABLE_GAP;
            }
            self.y = top + tallest;
            if n == 0 {
                self.page.path(&self.document, &[(left - 3.0, self.y), (left + width + 3.0, self.y)], false, Paint::stroke(rule(), 0.8));
            }
        }
        self.y += 8.0;
    }

    /// A picture, at its own size (a pixel to three quarters of a point, as a screen at 96 dots an inch has it), or
    /// smaller to fit.
    fn picture(&mut self, picture: crate::pictures::Picture, left: f32, width: f32) {
        let (mut w, mut h) = (picture.width as f32 * 0.75, picture.height as f32 * 0.75);
        if w > width {
            h *= width / w;
            w = width;
        }
        let tallest = self.bottom() - TOP;
        if h > tallest {
            w *= tallest / h;
            h = tallest;
        }
        self.room_for(h);
        let index = self.document.add_picture(crate::pdf::Picture { width: picture.width, height: picture.height, rgba: picture.rgba });
        self.page.picture(&self.document, index, left, self.y, w, h);
        self.y += h + 8.0;
    }

    fn diagram(&mut self, source: &str, left: f32, width: f32) {
        match &*crate::mermaid::cached(source) {
            Ok(scene) => {
                let tallest = self.bottom() - TOP;
                let scale = (width / scene.width).min(tallest / scene.height).min(1.0);
                self.room_for(scene.height * scale);
                let left = left + (width - scene.width * scale) / 2.0;
                crate::diagram::paint_pdf(&mut self.page, &self.document, scene, left, self.y, scale);
                self.y += scene.height * scale + 8.0;
            }
            Err(why) => {
                self.words(&[Span { text: why.clone(), italic: true, ..Span::default() }], left, width, BODY, false, faint());
                self.code(source, left, width);
            }
        }
    }

    fn block(&mut self, block: &Block, anchor: Option<&String>, top_level: u8) {
        let shift = block.indent as f32 * 18.0 + block.quote as f32 * 12.0;
        let left = LEFT + shift;
        let width = self.width() - shift;
        let start_page = self.page_number();
        let start_y = self.y;
        match &block.kind {
            BlockKind::Heading(level) => {
                let size = heading_size(*level);
                self.y += if *level <= 2 { 10.0 } else { 6.0 };
                // A heading isn't left alone at the foot of a page.
                self.room_for(size * LEADING * 3.0);
                let (page, y) = (self.page_number(), self.y);
                if let Some(anchor) = anchor {
                    self.anchors.insert(anchor.clone(), (page, y));
                }
                let title: String = block.spans.iter().map(|s| s.text.as_str()).collect();
                self.document.bookmarks.push(Bookmark { depth: usize::from(level.saturating_sub(top_level)), title: title.trim().to_string(), page, y });
                self.words(&block.spans, left, width, size, true, ink());
                if *level <= 2 {
                    self.page.path(&self.document, &[(left, self.y + 2.0), (left + width, self.y + 2.0)], false, Paint::stroke(rule(), 0.8));
                    self.y += 4.0;
                }
                self.y += 4.0;
            }
            BlockKind::Paragraph => {
                let marker_width = 16.0;
                if let Some(marker) = &block.marker {
                    self.room_for(BODY * LEADING);
                    let marker_x = left - 2.0;
                    let at = self.y + BODY * 0.95;
                    self.page.text(&self.document, marker_x, at, Font::Regular, BODY, faint(), marker);
                    let shift = crate::pdf::text_width(marker, Font::Regular, BODY).max(marker_width - 6.0) + 6.0;
                    self.words(&block.spans, left + shift, width - shift, BODY, false, ink());
                } else {
                    self.words(&block.spans, left, width, BODY, false, ink());
                }
                self.y += 5.0;
            }
            BlockKind::Code => {
                let text: String = block.spans.iter().map(|s| s.text.as_str()).collect();
                self.code(&text, left, width);
            }
            BlockKind::Rule => {
                self.room_for(12.0);
                self.y += 6.0;
                self.page.path(&self.document, &[(left, self.y), (left + width, self.y)], false, Paint::stroke(rule(), 0.8));
                self.y += 6.0;
            }
            BlockKind::Table(rows) => self.table(rows, left, width),
            BlockKind::Picture(path, alt) => match self.folder.and_then(|f| crate::preview::read_local_picture(&f.join(path))) {
                Some(picture) => self.picture(picture, left, width),
                None => self.words(&[Span { text: format!("[image: {alt}]"), italic: true, ..Span::default() }], left, width, BODY, false, faint()),
            },
            BlockKind::Diagram(source) => self.diagram(source, left, width),
        }
        // A quote's bar down its left, on each page it reached.
        if block.quote > 0 {
            for depth in 0..block.quote {
                let x = LEFT + block.indent as f32 * 18.0 + depth as f32 * 12.0 - 8.0;
                let from = if self.page_number() == start_page { start_y } else { TOP };
                self.page.path(&self.document, &[(x, from), (x, self.y - 4.0)], false, Paint::stroke(rule(), 2.0));
            }
        }
    }
}

/// The preview's blocks as a PDF: its bytes. `folder` is where the previewed file is, for its pictures.
pub fn make(blocks: &[Block], folder: Option<&Path>, title: &str) -> Vec<u8> {
    let mut writer = Writer { document: Document::a4(title), page: Page::default(), y: TOP, folder, heading_links: Vec::new(), anchors: HashMap::new() };
    let anchors = crate::preview::heading_anchors(blocks);
    let top_level = blocks.iter().filter_map(|b| if let BlockKind::Heading(level) = b.kind { Some(level) } else { None }).min().unwrap_or(1);
    for (block, anchor) in blocks.iter().zip(&anchors) {
        writer.block(block, anchor.as_ref(), top_level);
    }
    writer.new_page();
    let mut document = writer.document;
    // A last page left empty by a break just before the end isn't kept.
    while document.pages.len() > 1 && document.pages.last().is_some_and(Page::is_empty) {
        document.pages.pop();
    }
    for (page, rect, anchor) in writer.heading_links {
        if let (Some((to_page, y)), Some(on)) = (writer.anchors.get(&anchor), document.pages.get_mut(page)) {
            on.link(rect[0], rect[1], rect[2], rect[3], Target::Place(*to_page, (y - 6.0).max(0.0)));
        }
    }
    let total = document.pages.len();
    let (width, height) = (document.width, document.height);
    let mut pages = std::mem::take(&mut document.pages);
    for (n, page) in pages.iter_mut().enumerate() {
        let words = crate::i18n::tf!("Page {} of {total}", n + 1);
        let w = crate::pdf::text_width(&words, Font::Regular, 8.5);
        page.text(&document, (width - w) / 2.0, height - 32.0, Font::Regular, 8.5, faint(), &words);
    }
    document.pages = pages;
    document.to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str) -> Vec<Span> {
        vec![Span { text: text.to_string(), ..Span::default() }]
    }

    #[test]
    fn words_wrap_within_the_width_and_long_ones_are_broken() {
        let lines = lines_of(&spans("one two three four five six seven eight nine ten"), 10.0, false, 80.0);
        assert!(lines.len() > 2);
        for line in &lines {
            let end = line.last().map_or(0.0, |(x, p)| x + p.width);
            assert!(end <= 80.0 + 0.01, "{line:?}");
        }
        let long = lines_of(&spans(&"x".repeat(200)), 10.0, false, 50.0);
        assert!(long.len() > 3 && long.iter().all(|l| l.len() == 1 && l[0].1.width <= 50.0));
        let broken = lines_of(&spans("a\nb"), 10.0, false, 500.0);
        assert_eq!(broken.len(), 2, "a hard break starts a new line");
        // Styles keep their own pieces, with the space between them kept.
        let styled = vec![Span { text: "plain ".into(), ..Span::default() }, Span { text: "bold".into(), strong: true, ..Span::default() }];
        let line = &lines_of(&styled, 10.0, false, 500.0)[0];
        assert_eq!(line.len(), 2);
        assert_eq!(line[1].1.font, Font::Bold);
        assert!(line[1].0 > line[0].1.width + 1.0);
    }

    #[test]
    fn a_long_document_goes_over_pages_with_its_bookmarks_and_links() {
        let mut text = String::from("# Guide\n\n- [Later](#later)\n\n");
        for n in 0..80 {
            text += &format!("Paragraph {n} with **bold**, *italic*, `code` and [a link](https://example.org/{n}).\n\n");
        }
        text += "## Later\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n```\ncode line\n```\n\n```mermaid\ngraph LR\nA-->B\n```\n\n> quoted\n\n---\n";
        let blocks = crate::preview::render(crate::preview::Kind::Markdown, &text);
        let bytes = make(&blocks, None, "Guide");
        let as_text = String::from_utf8_lossy(&bytes);
        let pages = as_text.matches("/Type /Page ").count();
        assert!(pages >= 3, "{pages} pages");
        assert!(as_text.contains("/Outlines"));
        assert!(as_text.contains("/URI (https://example.org/79)"));
        assert!(as_text.contains("/Dest ["), "the contents' link goes to its heading");
    }

    #[test]
    fn the_pdf_reads_back_with_its_words_where_a_reader_is_installed() {
        let Ok(found) = std::process::Command::new("pdftotext").arg("-v").output() else { return };
        let _ = found;
        let text = "# Caf\u{E9} guide\n\nSome **bold** words.\n\n```mermaid\nsequenceDiagram\nAlice->>Bob: Hello\n```\n";
        let blocks = crate::preview::render(crate::preview::Kind::Markdown, text);
        let folder = std::env::temp_dir().join(format!("limoniter-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("guide.pdf");
        std::fs::write(&path, make(&blocks, None, "Caf\u{E9}")).unwrap();
        let out = std::process::Command::new("pdftotext").arg(&path).arg("-").output().unwrap();
        let words = String::from_utf8_lossy(&out.stdout);
        let _ = std::fs::remove_dir_all(&folder);
        assert!(words.contains("Caf\u{E9} guide"), "{words}");
        assert!(words.contains("Some bold words."));
        assert!(words.contains("Alice") && words.contains("Hello"));
        assert!(words.contains("Page 1 of 1"));
    }
}
