//! Code as a picture: the selection or the file, coloured as it's shown, on the theme's background with a margin
//! and line numbers if wished, made into a PNG to save or copy (as CodeSnap and Carbon make them). It's written
//! as an SVG of coloured text and drawn with resvg, in the editor's own font.

use eframe::egui::Color32;

/// One line: its number, and its text in runs of colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub number: usize,
    pub runs: Vec<(String, Color32)>,
}

/// How the picture looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub background: Color32,
    pub numbers_colour: Color32,
    pub font_size: f32,
    /// How wide a character is, and how tall a line, at that size.
    pub char_width: f32,
    pub line_height: f32,
    pub margin: f32,
    pub line_numbers: bool,
    /// Drawn this many times as large, for a sharp picture on a high-resolution screen.
    pub scale: f32,
}

fn hex(colour: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", colour.r(), colour.g(), colour.b())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The lines as an SVG: a rounded panel of the background colour, the numbers in a column at the left, and each
/// line's runs of colour as spans, every character a character's width apart.
pub fn svg(lines: &[Line], style: &Style) -> String {
    let widest = lines.iter().map(|l| l.runs.iter().map(|(t, _)| t.chars().count()).sum::<usize>()).max().unwrap_or(0);
    let digits = lines.iter().map(|l| l.number.to_string().len()).max().unwrap_or(1);
    let gutter = if style.line_numbers { (digits as f32 + 2.0) * style.char_width } else { 0.0 };
    let width = style.margin * 2.0 + gutter + widest.max(1) as f32 * style.char_width;
    let height = style.margin * 2.0 + lines.len().max(1) as f32 * style.line_height;
    let (scaled_width, scaled_height) = ((width * style.scale).ceil(), (height * style.scale).ceil());
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{scaled_width}\" height=\"{scaled_height}\" viewBox=\"0 0 {width} {height}\" font-family=\"monospace\" font-size=\"{}\" xml:space=\"preserve\">\n<rect width=\"{width}\" height=\"{height}\" rx=\"{}\" fill=\"{}\"/>\n",
        style.font_size,
        (style.margin * 0.5).min(12.0),
        hex(style.background)
    );
    // Each line's baseline sits about four-fifths of the way down its row.
    let baseline = |row: usize| style.margin + row as f32 * style.line_height + style.line_height * 0.78;
    for (row, line) in lines.iter().enumerate() {
        let y = baseline(row);
        if style.line_numbers {
            let x = style.margin + (digits as f32) * style.char_width;
            out.push_str(&format!("<text x=\"{x}\" y=\"{y}\" text-anchor=\"end\" fill=\"{}\">{}</text>\n", hex(style.numbers_colour), line.number));
        }
        // Each run placed at its own column, so the text keeps to the grid whatever the font's spacing.
        let mut column = 0usize;
        for (text, colour) in &line.runs {
            let length = text.chars().count();
            if !text.trim().is_empty() {
                let x = style.margin + gutter + column as f32 * style.char_width;
                let text_length = length as f32 * style.char_width;
                out.push_str(&format!("<text x=\"{x}\" y=\"{y}\" fill=\"{}\" textLength=\"{text_length}\" lengthAdjust=\"spacing\">{}</text>\n", hex(*colour), escape(text)));
            }
            column += length;
        }
    }
    out.push_str("</svg>\n");
    out
}

/// The SVG drawn, in the font given (the editor's), as a picture.
pub fn render(svg: &str, font: &[u8]) -> Result<crate::pictures::Picture, String> {
    let mut fonts = resvg::usvg::fontdb::Database::new();
    fonts.load_font_data(font.to_vec());
    let family = fonts.faces().next().and_then(|face| face.families.first()).map(|(name, _)| name.clone());
    if let Some(family) = family {
        fonts.set_monospace_family(family);
    }
    let options = resvg::usvg::Options { fontdb: std::sync::Arc::new(fonts), ..resvg::usvg::Options::default() };
    let tree = resvg::usvg::Tree::from_str(svg, &options).map_err(|e| e.to_string())?;
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height()).ok_or_else(|| crate::i18n::tr("That's too large to make a picture of.").to_string())?;
    resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Ok(crate::pictures::Picture { width: size.width() as usize, height: size.height() as usize, rgba, aspect: 1.0, info: Vec::new(), cycling: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> Style {
        Style { background: Color32::from_rgb(30, 30, 40), numbers_colour: Color32::GRAY, font_size: 14.0, char_width: 8.4, line_height: 20.0, margin: 24.0, line_numbers: true, scale: 1.0 }
    }

    #[test]
    fn code_is_drawn_on_its_background_in_its_colours() {
        let lines = vec![
            Line { number: 9, runs: vec![("let".into(), Color32::from_rgb(255, 0, 0)), (" x = 1;".into(), Color32::WHITE)] },
            Line { number: 10, runs: vec![("a < b && c".into(), Color32::WHITE)] },
        ];
        let text = svg(&lines, &style());
        assert!(text.contains(">let</text>") && text.contains("fill=\"#ff0000\""), "{text}");
        assert!(text.contains("a &lt; b &amp;&amp; c"), "escaped");
        assert!(text.contains(">10</text>"), "line numbers");
        // The panel is as wide as the margins, the numbers' column and the longest line.
        assert!(text.contains(&format!("width=\"{}\"", 24.0 * 2.0 + 4.0 * 8.4 + 10.0 * 8.4)), "{text}");
        let font = &eframe::egui::FontDefinitions::default().font_data["Hack"];
        let picture = render(&text, (*font.font).as_ref()).unwrap();
        assert_eq!(&picture.rgba[(picture.width * 2 + picture.width / 2) * 4..][..3], &[30, 30, 40], "the background");
        // Somewhere in the first line is the keyword's red.
        let red = picture.rgba.chunks(4).any(|p| p[0] > 200 && p[1] < 60 && p[2] < 60);
        assert!(red, "the keyword is drawn in its colour");
    }
}
