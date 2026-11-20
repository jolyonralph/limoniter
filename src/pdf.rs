//! A small PDF writer: pages of text in PDF's own standard fonts (Helvetica and Courier, which every PDF reader
//! has, so nothing needs to be put in the file), lines, boxes and shapes, pictures, links to web addresses and to
//! places in the document, and bookmarks. Text goes in the Windows-1252 set the standard fonts are read in, so a
//! character outside it shows as a question mark.
//!
//! Places are given from the page's top left, in points (a 72nd of an inch), as the preview lays things out;
//! they're turned round to PDF's own way, up from the bottom, as the page is written.

use std::fmt::Write as _;

/// The fonts: Helvetica in its four styles, and Courier for code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Font {
    Regular,
    Bold,
    Italic,
    BoldItalic,
    Mono,
    MonoBold,
}

impl Font {
    const ALL: [Font; 6] = [Font::Regular, Font::Bold, Font::Italic, Font::BoldItalic, Font::Mono, Font::MonoBold];

    fn base_name(self) -> &'static str {
        match self {
            Font::Regular => "Helvetica",
            Font::Bold => "Helvetica-Bold",
            Font::Italic => "Helvetica-Oblique",
            Font::BoldItalic => "Helvetica-BoldOblique",
            Font::Mono => "Courier",
            Font::MonoBold => "Courier-Bold",
        }
    }

    fn index(self) -> usize {
        Font::ALL.iter().position(|f| *f == self).unwrap_or(0)
    }

    pub fn styled(bold: bool, italic: bool, mono: bool) -> Font {
        match (mono, bold, italic) {
            (true, true, _) => Font::MonoBold,
            (true, false, _) => Font::Mono,
            (false, true, true) => Font::BoldItalic,
            (false, true, false) => Font::Bold,
            (false, false, true) => Font::Italic,
            (false, false, false) => Font::Regular,
        }
    }
}

/// Helvetica's widths, in thousandths of the font's size, for Windows-1252's characters from the space up.
const HELVETICA: [u16; 224] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, //
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, //
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, //
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, //
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, //
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584, 0, //
    556, 0, 222, 556, 333, 1000, 556, 556, 333, 1000, 667, 333, 1000, 0, 611, 0, //
    0, 222, 222, 333, 333, 350, 556, 1000, 333, 1000, 500, 333, 944, 0, 500, 667, //
    278, 333, 556, 556, 556, 556, 260, 556, 333, 737, 370, 556, 584, 333, 737, 333, //
    400, 584, 333, 333, 333, 556, 537, 278, 333, 333, 365, 556, 834, 834, 834, 611, //
    667, 667, 667, 667, 667, 667, 1000, 722, 667, 667, 667, 667, 278, 278, 278, 278, //
    722, 722, 778, 778, 778, 778, 778, 584, 778, 722, 722, 722, 722, 667, 667, 611, //
    556, 556, 556, 556, 556, 556, 889, 500, 556, 556, 556, 556, 278, 278, 278, 278, //
    556, 556, 556, 556, 556, 556, 556, 584, 611, 556, 556, 556, 556, 500, 556, 500, //
];

const HELVETICA_BOLD: [u16; 224] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, //
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, //
    975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, //
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556, //
    333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, //
    611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584, 0, //
    556, 0, 278, 556, 500, 1000, 556, 556, 333, 1000, 667, 333, 1000, 0, 611, 0, //
    0, 278, 278, 500, 500, 350, 556, 1000, 333, 1000, 556, 333, 944, 0, 500, 667, //
    278, 333, 556, 556, 556, 556, 280, 556, 333, 737, 370, 556, 584, 333, 737, 333, //
    400, 584, 333, 333, 333, 611, 556, 278, 333, 333, 365, 556, 834, 834, 834, 611, //
    722, 722, 722, 722, 722, 722, 1000, 722, 667, 667, 667, 667, 278, 278, 278, 278, //
    722, 722, 778, 778, 778, 778, 778, 584, 778, 722, 722, 722, 722, 667, 667, 611, //
    556, 556, 556, 556, 556, 556, 889, 556, 556, 556, 556, 556, 278, 278, 278, 278, //
    611, 611, 611, 611, 611, 611, 611, 584, 611, 611, 611, 611, 611, 556, 611, 556, //
];

/// Windows-1252's characters from $80 to $9F, which aren't Latin-1's.
const WIN_ANSI_HIGH: [char; 32] = [
    '\u{20AC}', '\0', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}', '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\0', '\u{017D}', '\0', //
    '\0', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}', '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\0', '\u{017E}', '\u{0178}',
];

/// A character's byte in Windows-1252, if it has one. A tab is a space.
pub fn win_ansi(c: char) -> Option<u8> {
    match c {
        '\t' => Some(b' '),
        ' '..='~' => Some(c as u8),
        '\u{A0}'..='\u{FF}' => Some(c as u8),
        _ => WIN_ANSI_HIGH.iter().position(|h| *h == c && c != '\0').map(|at| 0x80 + at as u8),
    }
}

/// What a character outside Windows-1252 is written as instead: something near it where there is one.
fn stand_in(c: char) -> &'static str {
    match c {
        '\u{2610}' => "[ ]",
        '\u{2611}' | '\u{2612}' => "[x]",
        '\u{2192}' | '\u{27F6}' => "->",
        '\u{2190}' => "<-",
        '\u{2194}' => "<->",
        '\u{21D2}' => "=>",
        '\u{2212}' => "-",
        '\u{2264}' => "<=",
        '\u{2265}' => ">=",
        '\u{2260}' => "!=",
        '\u{2248}' => "~",
        '\u{25CF}' | '\u{25E6}' | '\u{25AA}' => "\u{2022}",
        '\u{2502}' | '\u{2503}' => "|",
        '\u{2500}' | '\u{2501}' => "-",
        '\u{2011}' | '\u{2010}' => "-",
        '\u{2002}' | '\u{2003}' | '\u{2009}' | '\u{202F}' | '\u{200B}' => " ",
        _ => "?",
    }
}

/// Text as the bytes the standard fonts read, with stand-ins for what Windows-1252 hasn't got.
pub fn encode(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        match win_ansi(c) {
            Some(byte) => out.push(byte),
            None if c.is_control() => {}
            None => out.extend(stand_in(c).chars().filter_map(win_ansi)),
        }
    }
    out
}

/// How wide text is in a font at a size, in points.
pub fn text_width(text: &str, font: Font, size: f32) -> f32 {
    let thousandths: u32 = encode(text)
        .iter()
        .map(|byte| match font {
            Font::Mono | Font::MonoBold => 600,
            Font::Bold | Font::BoldItalic => u32::from(HELVETICA_BOLD.get(usize::from(*byte).wrapping_sub(32)).copied().unwrap_or(556)),
            Font::Regular | Font::Italic => u32::from(HELVETICA.get(usize::from(*byte).wrapping_sub(32)).copied().unwrap_or(556)),
        })
        .sum();
    thousandths as f32 * size / 1000.0
}

/// A colour, each part from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub f32, pub f32, pub f32);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0.0, 0.0, 0.0);

    pub fn from_bytes(r: u8, g: u8, b: u8) -> Rgb {
        Rgb(f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0)
    }
}

/// Where a link goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Web(String),
    /// A page, counting from 0, and how far down it.
    Place(usize, f32),
}

/// A bookmark: its depth (from 0), its words, and the page and height it goes to.
#[derive(Clone, Debug, PartialEq)]
pub struct Bookmark {
    pub depth: usize,
    pub title: String,
    pub page: usize,
    pub y: f32,
}

/// A picture for the pages, kept once however often it's drawn.
pub struct Picture {
    pub width: usize,
    pub height: usize,
    /// Red, green, blue and opacity for each pixel, not multiplied together.
    pub rgba: Vec<u8>,
}

/// One page being drawn.
#[derive(Default)]
pub struct Page {
    ops: String,
    links: Vec<([f32; 4], Target)>,
}

/// How a shape is drawn: filled, outlined, or both.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paint {
    pub fill: Option<Rgb>,
    pub stroke: Option<Rgb>,
    pub width: f32,
    pub dashed: bool,
}

impl Paint {
    pub fn fill(colour: Rgb) -> Paint {
        Paint { fill: Some(colour), stroke: None, width: 0.0, dashed: false }
    }

    pub fn stroke(colour: Rgb, width: f32) -> Paint {
        Paint { fill: None, stroke: Some(colour), width, dashed: false }
    }
}

fn number(value: f32) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded == rounded.trunc() { format!("{}", rounded as i64) } else { format!("{rounded}") }
}

impl Page {
    /// Whether nothing has been drawn on it.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty() && self.links.is_empty()
    }

    /// The page's height, for turning places round: set by the document when the page is added.
    fn y(height: f32, y: f32) -> f32 {
        height - y
    }

    /// Words at a place: `y` is where the line of text sits (its baseline).
    pub fn text(&mut self, document: &Document, x: f32, y: f32, font: Font, size: f32, colour: Rgb, text: &str) {
        let bytes = encode(text);
        if bytes.is_empty() {
            return;
        }
        let mut literal = String::with_capacity(bytes.len() + 2);
        literal.push('(');
        for byte in bytes {
            match byte {
                b'(' | b')' | b'\\' => {
                    literal.push('\\');
                    literal.push(char::from(byte));
                }
                32..=126 => literal.push(char::from(byte)),
                _ => {
                    let _ = write!(literal, "\\{byte:03o}");
                }
            }
        }
        literal.push(')');
        let _ = writeln!(
            self.ops,
            "BT /F{} {} Tf {} {} {} rg {} {} Td {literal} Tj ET",
            font.index() + 1,
            number(size),
            number(colour.0),
            number(colour.1),
            number(colour.2),
            number(x),
            number(Self::y(document.height, y))
        );
    }

    fn paint_start(&mut self, paint: &Paint) {
        self.ops.push_str("q ");
        if let Some(Rgb(r, g, b)) = paint.fill {
            let _ = write!(self.ops, "{} {} {} rg ", number(r), number(g), number(b));
        }
        if let Some(Rgb(r, g, b)) = paint.stroke {
            let _ = write!(self.ops, "{} {} {} RG {} w ", number(r), number(g), number(b), number(paint.width.max(0.1)));
            if paint.dashed {
                self.ops.push_str("[3 2] 0 d ");
            }
        }
        self.ops.push_str("1 j 1 J ");
    }

    fn paint_end(&mut self, paint: &Paint, closed: bool) {
        let op = match (paint.fill.is_some(), paint.stroke.is_some(), closed) {
            (true, true, _) => "B",
            (true, false, _) => "f",
            (false, true, true) => "s",
            (false, true, false) => "S",
            (false, false, _) => "n",
        };
        let _ = writeln!(self.ops, "{op} Q");
    }

    /// A line through some points, or a shape if it's closed.
    pub fn path(&mut self, document: &Document, points: &[(f32, f32)], closed: bool, paint: Paint) {
        let Some(first) = points.first() else { return };
        self.paint_start(&paint);
        let _ = write!(self.ops, "{} {} m ", number(first.0), number(Self::y(document.height, first.1)));
        for (x, y) in &points[1..] {
            let _ = write!(self.ops, "{} {} l ", number(*x), number(Self::y(document.height, *y)));
        }
        if closed {
            self.ops.push_str("h ");
        }
        self.paint_end(&paint, closed);
    }

    /// A box, with its corners rounded by `radius`.
    pub fn rect(&mut self, document: &Document, x: f32, y: f32, width: f32, height: f32, radius: f32, paint: Paint) {
        let radius = radius.min(width / 2.0).min(height / 2.0).max(0.0);
        if radius < 0.5 {
            self.paint_start(&paint);
            let _ = write!(self.ops, "{} {} {} {} re ", number(x), number(Self::y(document.height, y + height)), number(width), number(height));
            self.paint_end(&paint, true);
            return;
        }
        // Each corner a quarter of an ellipse, drawn as a Bézier curve.
        let k = radius * 0.552_285;
        let (left, right, top, bottom) = (x, x + width, Self::y(document.height, y), Self::y(document.height, y + height));
        self.paint_start(&paint);
        let n = number;
        let _ = write!(
            self.ops,
            "{} {} m {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c h ",
            n(left + radius),
            n(top),
            n(right - radius),
            n(top),
            n(right - radius + k),
            n(top),
            n(right),
            n(top - radius + k),
            n(right),
            n(top - radius),
            n(right),
            n(bottom + radius),
            n(right),
            n(bottom + radius - k),
            n(right - radius + k),
            n(bottom),
            n(right - radius),
            n(bottom),
            n(left + radius),
            n(bottom),
            n(left + radius - k),
            n(bottom),
            n(left),
            n(bottom + radius - k),
            n(left),
            n(bottom + radius),
            n(left),
            n(top - radius),
            n(left),
            n(top - radius + k),
            n(left + radius - k),
            n(top),
            n(left + radius),
            n(top)
        );
        self.paint_end(&paint, true);
    }

    /// An ellipse in the box given by its centre and radii.
    pub fn ellipse(&mut self, document: &Document, cx: f32, cy: f32, rx: f32, ry: f32, paint: Paint) {
        let (kx, ky) = (rx * 0.552_285, ry * 0.552_285);
        let cy = Self::y(document.height, cy);
        let n = number;
        self.paint_start(&paint);
        let _ = write!(
            self.ops,
            "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c h ",
            n(cx + rx),
            n(cy),
            n(cx + rx),
            n(cy + ky),
            n(cx + kx),
            n(cy + ry),
            n(cx),
            n(cy + ry),
            n(cx - kx),
            n(cy + ry),
            n(cx - rx),
            n(cy + ky),
            n(cx - rx),
            n(cy),
            n(cx - rx),
            n(cy - ky),
            n(cx - kx),
            n(cy - ry),
            n(cx),
            n(cy - ry),
            n(cx + kx),
            n(cy - ry),
            n(cx + rx),
            n(cy - ky),
            n(cx + rx),
            n(cy)
        );
        self.paint_end(&paint, true);
    }

    /// A picture the document holds, drawn into a box.
    pub fn picture(&mut self, document: &Document, index: usize, x: f32, y: f32, width: f32, height: f32) {
        let _ = writeln!(self.ops, "q {} 0 0 {} {} {} cm /Im{} Do Q", number(width), number(height), number(x), number(Self::y(document.height, y + height)), index + 1);
    }

    /// A part of the page that can be clicked to go somewhere.
    pub fn link(&mut self, x: f32, y: f32, width: f32, height: f32, target: Target) {
        self.links.push(([x, y, width, height], target));
    }
}

/// A whole document: its pages, pictures and bookmarks, all one size of page.
pub struct Document {
    pub width: f32,
    pub height: f32,
    pub title: String,
    pub pages: Vec<Page>,
    pictures: Vec<Picture>,
    pub bookmarks: Vec<Bookmark>,
}

/// A text string for the document's own information and bookmarks, in UTF-16 so any language reads.
fn info_string(text: &str) -> String {
    let mut out = String::from("<FEFF");
    for unit in text.encode_utf16() {
        let _ = write!(out, "{unit:04X}");
    }
    out.push('>');
    out
}

fn deflate(bytes: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(bytes, 6)
}

impl Document {
    /// A4: 210 by 297 millimetres.
    pub fn a4(title: &str) -> Document {
        Document { width: 595.28, height: 841.89, title: title.to_string(), pages: Vec::new(), pictures: Vec::new(), bookmarks: Vec::new() }
    }

    /// Keeps a picture, for drawing on the pages by the number this gives.
    pub fn add_picture(&mut self, picture: Picture) -> usize {
        self.pictures.push(picture);
        self.pictures.len() - 1
    }

    /// The file's bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        // Objects: 1 the catalogue, 2 the page tree, 3 to 8 the fonts, then each picture (and its opacity), each
        // page and its contents, and the bookmarks.
        let mut objects: Vec<Vec<u8>> = Vec::new();
        let fonts_from = 3;
        let pictures_from = fonts_from + Font::ALL.len();
        let pages_from = pictures_from + self.pictures.len() * 2;
        let outline_at = pages_from + self.pages.len() * 2;
        let page_object = |page: usize| pages_from + page * 2;
        let has_outline = !self.bookmarks.is_empty();

        let mut catalogue = String::from("<< /Type /Catalog /Pages 2 0 R");
        if has_outline {
            let _ = write!(catalogue, " /Outlines {outline_at} 0 R /PageMode /UseOutlines");
        }
        catalogue.push_str(" >>");
        objects.push(catalogue.into_bytes());
        let kids: Vec<String> = (0..self.pages.len()).map(|p| format!("{} 0 R", page_object(p))).collect();
        objects.push(format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), self.pages.len()).into_bytes());
        for font in Font::ALL {
            objects.push(format!("<< /Type /Font /Subtype /Type1 /BaseFont /{} /Encoding /WinAnsiEncoding >>", font.base_name()).into_bytes());
        }
        for (n, picture) in self.pictures.iter().enumerate() {
            let pixels = picture.width * picture.height;
            let rgb: Vec<u8> = picture.rgba.chunks(4).take(pixels).flat_map(|p| [p[0], p[1], p[2]]).collect();
            let alpha: Vec<u8> = picture.rgba.chunks(4).take(pixels).map(|p| p[3]).collect();
            let packed = deflate(&rgb);
            let mut object = format!(
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /SMask {} 0 R /Length {} >>\nstream\n",
                picture.width,
                picture.height,
                pictures_from + n * 2 + 1,
                packed.len()
            )
            .into_bytes();
            object.extend_from_slice(&packed);
            object.extend_from_slice(b"\nendstream");
            objects.push(object);
            let packed = deflate(&alpha);
            let mut mask = format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n", picture.width, picture.height, packed.len()).into_bytes();
            mask.extend_from_slice(&packed);
            mask.extend_from_slice(b"\nendstream");
            objects.push(mask);
        }
        let fonts: String = (0..Font::ALL.len()).map(|f| format!("/F{} {} 0 R ", f + 1, fonts_from + f)).collect();
        let images: String = (0..self.pictures.len()).map(|p| format!("/Im{} {} 0 R ", p + 1, pictures_from + p * 2)).collect();
        for (n, page) in self.pages.iter().enumerate() {
            let mut annotations = String::new();
            for ([x, y, width, height], target) in &page.links {
                let rect = format!("[{} {} {} {}]", number(*x), number(self.height - y - height), number(x + width), number(self.height - y));
                let action = match target {
                    Target::Web(address) => format!("/A << /S /URI /URI ({}) >>", address.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)")),
                    Target::Place(page, y) => format!("/Dest [{} 0 R /XYZ 0 {} 0]", page_object((*page).min(self.pages.len().saturating_sub(1))), number(self.height - y)),
                };
                let _ = write!(annotations, "<< /Type /Annot /Subtype /Link /Rect {rect} /Border [0 0 0] {action} >> ");
            }
            let annots = if annotations.is_empty() { String::new() } else { format!(" /Annots [{annotations}]") };
            let x_objects = if images.is_empty() { String::new() } else { format!(" /XObject << {images}>>") };
            objects.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << {fonts}>>{x_objects} >> /Contents {} 0 R{annots} >>",
                    number(self.width),
                    number(self.height),
                    page_object(n) + 1
                )
                .into_bytes(),
            );
            let packed = deflate(page.ops.as_bytes());
            let mut contents = format!("<< /Filter /FlateDecode /Length {} >>\nstream\n", packed.len()).into_bytes();
            contents.extend_from_slice(&packed);
            contents.extend_from_slice(b"\nendstream");
            objects.push(contents);
        }
        if has_outline {
            objects.extend(self.outline_objects(outline_at, &page_object));
        }
        let info_at = objects.len() + 1;
        objects.push(format!("<< /Title {} /Producer (Limoniter) >>", info_string(&self.title)).into_bytes());

        let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (n, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", n + 1).as_bytes());
            out.extend_from_slice(object);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
        for offset in offsets {
            let _ = writeln!(table, "{offset:010} 00000 n ");
        }
        let _ = write!(table, "trailer\n<< /Size {} /Root 1 0 R /Info {info_at} 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1);
        out.extend_from_slice(table.as_bytes());
        out
    }

    /// The bookmarks as PDF's outline: a tree, each entry linked to its parent, its first and last children and its
    /// neighbours.
    fn outline_objects(&self, outline_at: usize, page_object: &dyn Fn(usize) -> usize) -> Vec<Vec<u8>> {
        let count = self.bookmarks.len();
        let object = |n: usize| outline_at + 1 + n;
        // Each bookmark's parent: the nearest before it that's shallower.
        let mut parents: Vec<Option<usize>> = Vec::with_capacity(count);
        for (n, mark) in self.bookmarks.iter().enumerate() {
            parents.push((0..n).rev().find(|&before| self.bookmarks[before].depth < mark.depth));
        }
        let children = |parent: Option<usize>| -> Vec<usize> { (0..count).filter(|&n| parents[n] == parent).collect() };
        let mut out = Vec::with_capacity(count + 1);
        let top = children(None);
        out.push(format!("<< /Type /Outlines /First {} 0 R /Last {} 0 R /Count {} >>", object(top[0]), object(*top.last().unwrap_or(&top[0])), top.len()).into_bytes());
        for (n, mark) in self.bookmarks.iter().enumerate() {
            let siblings = children(parents[n]);
            let place = siblings.iter().position(|s| *s == n).unwrap_or(0);
            let mut entry = format!(
                "<< /Title {} /Parent {} 0 R /Dest [{} 0 R /XYZ 0 {} 0]",
                info_string(&mark.title),
                parents[n].map_or(outline_at, object),
                page_object(mark.page.min(self.pages.len().saturating_sub(1))),
                number(self.height - mark.y)
            );
            if place > 0 {
                let _ = write!(entry, " /Prev {} 0 R", object(siblings[place - 1]));
            }
            if let Some(next) = siblings.get(place + 1) {
                let _ = write!(entry, " /Next {} 0 R", object(*next));
            }
            let mine = children(Some(n));
            if let (Some(first), Some(last)) = (mine.first(), mine.last()) {
                let _ = write!(entry, " /First {} 0 R /Last {} 0 R /Count -{}", object(*first), object(*last), mine.len());
            }
            entry.push_str(" >>");
            out.push(entry.into_bytes());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_measured_and_written_in_windows_1252() {
        // "Hello" in Helvetica: H 722, e 556, l 222, l 222, o 556.
        assert!((text_width("Hello", Font::Regular, 10.0) - 22.78).abs() < 0.01);
        assert!((text_width("abc", Font::Mono, 10.0) - 18.0).abs() < 0.01);
        assert!(text_width("Hello", Font::Bold, 10.0) > text_width("Hello", Font::Regular, 10.0));
        assert_eq!(encode("caf\u{E9} \u{20AC}5 \u{2014} \u{2610}"), b"caf\xE9 \x805 \x97 [ ]".to_vec());
        assert_eq!(encode("\u{4E2D}"), b"?".to_vec(), "what the standard fonts haven't got is a question mark");
    }

    #[test]
    fn a_document_has_its_pages_links_pictures_and_bookmarks() {
        let mut document = Document::a4("A test");
        let picture = document.add_picture(Picture { width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 0, 255, 128] });
        for n in 0..2 {
            let mut page = Page::default();
            page.text(&document, 72.0, 72.0, Font::Bold, 14.0, Rgb::BLACK, &format!("Page (one) {n}"));
            page.rect(&document, 72.0, 100.0, 100.0, 40.0, 6.0, Paint { fill: Some(Rgb(0.9, 0.9, 1.0)), stroke: Some(Rgb::BLACK), width: 1.0, dashed: true });
            page.ellipse(&document, 300.0, 300.0, 20.0, 10.0, Paint::stroke(Rgb::BLACK, 1.0));
            page.path(&document, &[(10.0, 10.0), (20.0, 20.0)], false, Paint::stroke(Rgb::BLACK, 1.0));
            page.picture(&document, picture, 72.0, 200.0, 20.0, 10.0);
            page.link(72.0, 60.0, 50.0, 14.0, if n == 0 { Target::Place(1, 72.0) } else { Target::Web("https://example.org/(x)".into()) });
            document.pages.push(page);
        }
        document.bookmarks = vec![
            Bookmark { depth: 0, title: "One".into(), page: 0, y: 72.0 },
            Bookmark { depth: 1, title: "One a".into(), page: 0, y: 120.0 },
            Bookmark { depth: 0, title: "Two \u{2014} \u{E9}".into(), page: 1, y: 72.0 },
        ];
        let bytes = document.to_bytes();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.ends_with("%%EOF\n"));
        assert!(text.contains("/Count 2 >>"), "two pages");
        assert!(text.contains("/BaseFont /Helvetica-Bold"));
        assert!(text.contains("/URI (https://example.org/\\(x\\))"));
        assert!(text.contains("/Subtype /Image /Width 2 /Height 1"));
        assert!(text.contains("/Outlines"));
        // Every object the cross-reference table names starts where it says.
        let xref_at: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        let table = String::from_utf8_lossy(&bytes[xref_at..]).to_string();
        assert!(table.starts_with("xref\n"));
        let mut checked = 0;
        for (n, line) in table.lines().skip(3).take_while(|l| l.ends_with(" n ")).enumerate() {
            let offset: usize = line[..10].parse().unwrap();
            assert!(bytes[offset..].starts_with(format!("{} 0 obj", n + 1).as_bytes()), "object {}", n + 1);
            checked += 1;
        }
        assert!(checked > 15);
    }
}
