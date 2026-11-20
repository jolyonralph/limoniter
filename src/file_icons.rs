//! Small badges for files by their kind, in the Explorer, on the tabs and in Go to file, as icon themes give VS
//! Code: a rounded square in the colour each language is known by, with a letter or two on it.

use eframe::egui::{self, Color32, Painter, Rect};

/// The letters and colour for a file, by its name.
pub fn badge(name: &str) -> (&'static str, Color32) {
    let lower = name.to_lowercase();
    let ending = lower.rsplit_once('.').map_or("", |(_, e)| e);
    let rgb = |r, g, b| Color32::from_rgb(r, g, b);
    match (lower.as_str(), ending) {
        ("makefile" | "gnumakefile", _) | (_, "mk") => ("MK", rgb(0x6d, 0x82, 0x86)),
        ("dockerfile", _) => ("DK", rgb(0x24, 0x96, 0xed)),
        ("cargo.toml" | "cargo.lock", _) => ("RS", rgb(0xde, 0xa5, 0x84)),
        ("readme" | "readme.md" | "readme.txt", _) => ("i", rgb(0x42, 0xa5, 0xf5)),
        ("license" | "licence" | "license.md" | "license.txt", _) => ("\u{A9}", rgb(0xc0, 0xa0, 0x30)),
        (_, "rs") => ("RS", rgb(0xde, 0xa5, 0x84)),
        (_, "py" | "pyw" | "pyi") => ("PY", rgb(0x35, 0x72, 0xa5)),
        (_, "js" | "mjs" | "cjs") => ("JS", rgb(0xd4, 0xb8, 0x30)),
        (_, "ts" | "mts" | "cts") => ("TS", rgb(0x31, 0x78, 0xc6)),
        (_, "jsx" | "tsx") => ("JSX", rgb(0x4f, 0xa8, 0xc8)),
        (_, "c") => ("C", rgb(0x55, 0x55, 0xaa)),
        (_, "h" | "hpp" | "hh" | "hxx") => ("H", rgb(0x8a, 0x6f, 0xbf)),
        (_, "cpp" | "cc" | "cxx" | "c++") => ("C++", rgb(0xf3, 0x4b, 0x7d)),
        (_, "cs") => ("C#", rgb(0x17, 0x86, 0x00)),
        (_, "java" | "class" | "jar") => ("JV", rgb(0xb0, 0x72, 0x19)),
        (_, "kt" | "kts") => ("KT", rgb(0xa9, 0x7b, 0xff)),
        (_, "go") => ("GO", rgb(0x00, 0xad, 0xd8)),
        (_, "swift") => ("SW", rgb(0xf0, 0x51, 0x38)),
        (_, "rb") => ("RB", rgb(0xcc, 0x34, 0x2d)),
        (_, "php") => ("PHP", rgb(0x4f, 0x5d, 0x95)),
        (_, "lua") => ("LUA", rgb(0x00, 0x00, 0x80)),
        (_, "pas" | "pp" | "dpr") => ("PAS", rgb(0xe3, 0xf1, 0x71)),
        (_, "html" | "htm" | "xhtml") => ("<>", rgb(0xe3, 0x4c, 0x26)),
        (_, "css" | "scss" | "sass" | "less") => ("#", rgb(0x56, 0x3d, 0x7c)),
        (_, "json" | "jsonc") => ("{}", rgb(0xcb, 0xa1, 0x3c)),
        (_, "xml" | "xsl" | "xsd" | "plist") => ("XML", rgb(0x0f, 0x8a, 0x5f)),
        (_, "yaml" | "yml") => ("YML", rgb(0xcb, 0x17, 0x1e)),
        (_, "toml" | "ini" | "cfg" | "conf") => ("CFG", rgb(0x6d, 0x82, 0x86)),
        (_, "md" | "markdown") => ("MD", rgb(0x42, 0x7c, 0xb0)),
        (_, "txt" | "text" | "log") => ("TXT", rgb(0x80, 0x80, 0x80)),
        (_, "sh" | "bash" | "zsh" | "fish") => ("$", rgb(0x4e, 0xaa, 0x25)),
        (_, "bat" | "cmd" | "ps1") => (">", rgb(0x01, 0x24, 0x56)),
        (_, "sql") => ("SQL", rgb(0xe3, 0x8c, 0x00)),
        (_, "svg") => ("SVG", rgb(0xff, 0xb1, 0x3b)),
        (_, "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" | "iff" | "ilbm" | "pcx") => ("IMG", rgb(0x26, 0xa6, 0x9a)),
        (_, "wav" | "mp3" | "ogg" | "flac" | "mod" | "xm" | "s3m" | "it" | "sid" | "ym" | "sndh") => ("\u{266A}", rgb(0xef, 0x53, 0x50)),
        (_, "s" | "asm" | "a65" | "a" | "z80" | "i" | "inc" | "x68") => ("ASM", rgb(0x6e, 0x4c, 0x13)),
        (_, "bas" | "bbc" | "prg" | "tap" | "tzx") => ("BAS", rgb(0x5c, 0x6b, 0xc0)),
        (_, "e") => ("E", rgb(0xd0, 0x5c, 0x1e)),
        (_, "adf" | "adz" | "dms" | "hdf" | "d64" | "d71" | "d81" | "atr" | "st" | "msa" | "dsk" | "trd" | "scl" | "ssd" | "dsd" | "img" | "ima" | "iso") => ("DSK", rgb(0x79, 0x55, 0x48)),
        (_, "zip" | "lha" | "lzh" | "gz" | "tgz" | "7z" | "rar" | "tar" | "xz" | "bz2") => ("ZIP", rgb(0xaf, 0xb4, 0x2b)),
        (_, "pdf") => ("PDF", rgb(0xd3, 0x2f, 0x2f)),
        (_, "bin" | "rom" | "exe" | "dll" | "so" | "o" | "obj") => ("01", rgb(0x60, 0x7d, 0x8b)),
        _ => ("", rgb(0x9e, 0x9e, 0x9e)),
    }
}

/// How wide a badge is for text of this size.
pub fn width(size: f32) -> f32 {
    (size * 1.25).round()
}

/// How many pixels across a file icon theme's pictures are drawn at, enough for a screen of twice the density.
const PICTURE_PIXELS: u32 = 40;

thread_local! {
    /// The pictures of a file icon theme drawn so far, by file, with the extensions' generation they're from; a
    /// picture that couldn't be drawn is remembered as None, so it isn't tried again every frame.
    static PICTURES: std::cell::RefCell<(u64, std::collections::HashMap<std::path::PathBuf, Option<egui::TextureHandle>>)> = std::cell::RefCell::new((0, std::collections::HashMap::new()));
}

thread_local! {
    /// Which picture each file name has, worked out once (for light and dark) while the extensions stay as they are.
    static CHOSEN: std::cell::RefCell<(u64, std::collections::HashMap<(String, bool), Option<std::path::PathBuf>>)> = std::cell::RefCell::new((0, std::collections::HashMap::new()));
}

fn icon_path(name: &str, light: bool) -> Option<std::path::PathBuf> {
    let generation = crate::vsix::generation();
    CHOSEN.with(|cell| {
        let mut cache = cell.borrow_mut();
        if cache.0 != generation {
            *cache = (generation, std::collections::HashMap::new());
        }
        // Until the grammars are loaded, a file's language isn't known, so nothing's kept from then.
        if crate::highlight::engine().is_none() {
            return crate::vsix::icon_for(name, light);
        }
        cache.1.entry((name.to_string(), light)).or_insert_with(|| crate::vsix::icon_for(name, light)).clone()
    })
}

/// An icon theme's picture (SVG or PNG) as a texture, drawn once.
fn picture(ctx: &egui::Context, path: &std::path::Path) -> Option<egui::TextureHandle> {
    let generation = crate::vsix::generation();
    PICTURES.with(|cell| {
        let mut cache = cell.borrow_mut();
        if cache.0 != generation {
            *cache = (generation, std::collections::HashMap::new());
        }
        cache
            .1
            .entry(path.to_path_buf())
            .or_insert_with(|| {
                let bytes = std::fs::read(path).ok()?;
                let image = if bytes.starts_with(b"\x89PNG") { png_image(&bytes)? } else { svg_image(&bytes, path.parent())? };
                Some(ctx.load_texture(format!("file-icon {}", path.display()), image, egui::TextureOptions::LINEAR))
            })
            .clone()
    })
}

fn svg_image(bytes: &[u8], folder: Option<&std::path::Path>) -> Option<egui::ColorImage> {
    let options = resvg::usvg::Options { resources_dir: folder.map(std::path::Path::to_path_buf), ..resvg::usvg::Options::default() };
    let tree = resvg::usvg::Tree::from_data(bytes, &options).ok()?;
    let size = tree.size();
    let scale = PICTURE_PIXELS as f32 / size.width().max(size.height()).max(1.0);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(PICTURE_PIXELS, PICTURE_PIXELS)?;
    let (dx, dy) = ((PICTURE_PIXELS as f32 - size.width() * scale) / 2.0, (PICTURE_PIXELS as f32 - size.height() * scale) / 2.0);
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy), &mut pixmap.as_mut());
    let rgba: Vec<u8> = pixmap.pixels().iter().flat_map(|p| [p.red(), p.green(), p.blue(), p.alpha()]).collect();
    Some(egui::ColorImage::from_rgba_premultiplied([PICTURE_PIXELS as usize, PICTURE_PIXELS as usize], &rgba))
}

fn png_image(bytes: &[u8]) -> Option<egui::ColorImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buffer).ok()?;
    let data = &buffer[..frame.buffer_size()];
    let rgba: Vec<u8> = match frame.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::GrayscaleAlpha => data.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        _ => return None,
    };
    Some(egui::ColorImage::from_rgba_unmultiplied([frame.width as usize, frame.height as usize], &rgba))
}

/// Draws a file's badge in a square at the left of `rect`, its middle on the rect's: the chosen file icon
/// theme's picture for it if there is one, or else Limoniter's own badge.
pub fn paint(painter: &Painter, rect: Rect, name: &str, size: f32) {
    let side = width(size) - 2.0;
    let square = Rect::from_center_size(egui::pos2(rect.min.x + side / 2.0 + 1.0, rect.center().y), egui::vec2(side, side));
    if let Some(path) = icon_path(name, crate::theme::is_light())
        && let Some(texture) = picture(painter.ctx(), &path)
    {
        painter.image(texture.id(), square, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        return;
    }
    let (letters, colour) = badge(name);
    if letters.is_empty() {
        // A plain page, for a file of no known kind.
        painter.rect_stroke(square.shrink(2.0), 1.5, egui::Stroke::new(1.0, colour), egui::StrokeKind::Inside);
        return;
    }
    painter.rect_filled(square, 3.0, colour);
    let shade = colour.r() as f32 * 0.299 + colour.g() as f32 * 0.587 + colour.b() as f32 * 0.114;
    let ink = if shade > 150.0 { Color32::from_rgb(30, 30, 30) } else { Color32::WHITE };
    let font = egui::FontId::proportional((side * if letters.chars().count() >= 3 { 0.42 } else { 0.58 }).max(6.0));
    painter.text(square.center(), egui::Align2::CENTER_CENTER, letters, font, ink);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_get_the_badge_of_their_kind() {
        assert_eq!(badge("main.RS").0, "RS");
        assert_eq!(badge("Makefile").0, "MK");
        assert_eq!(badge("intro.s").0, "ASM");
        assert_eq!(badge("game.adf").0, "DSK");
        assert_eq!(badge("notes").0, "", "no kind, a plain page");
    }
}
