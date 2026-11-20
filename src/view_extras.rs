//! Touches to how text is drawn that don't change the text: comment kinds coloured (TODO, FIXME, NOTE and comments
//! that begin with ! or ?), indentation guides, problems written at the end of their line, and colours written in
//! the text (#rrggbb, rgb(), hsl() and the like) shown as what they are, with a picker to change one.

use eframe::egui::Color32;

/// Words in comments that stand out, and the colour tag (1 red to 6 purple) each is shown in.
const KEYWORDS: [(&str, u8); 7] = [("TODO", 2), ("FIXME", 1), ("BUG", 1), ("XXX", 1), ("HACK", 6), ("NOTE", 5), ("REVIEW", 5)];

/// How a comment's own kind is shown, by what it begins with after its marker (as Better Comments does): ! a
/// warning, ? a question, * a highlight. The colour tag for each.
fn kind_of(rest: &str) -> Option<u8> {
    match rest.chars().next()? {
        '!' => Some(1),
        '?' => Some(5),
        '*' if !rest.starts_with("**") => Some(4),
        _ => None,
    }
}

/// The comment markers a comment's text begins with, for finding what comes after them.
fn after_marker(text: &str) -> usize {
    let trimmed = text.trim_start();
    let lead = text.len() - trimmed.len();
    for marker in ["///", "//!", "//", "/**", "/*", "--", "#", ";;", ";", "'", "REM ", "rem ", "%", "*"] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            let rest_trimmed = rest.trim_start();
            return lead + marker.len() + (rest.len() - rest_trimmed.len());
        }
    }
    lead
}

/// The stretches of a line's shown text to colour as comment kinds, by column: (from, to, colour tag).
/// `comments` are the line's comment stretches, by column.
pub fn comment_kinds(shown: &str, comments: &[(usize, usize)]) -> Vec<(usize, usize, u8)> {
    let chars: Vec<char> = shown.chars().collect();
    let mut found = Vec::new();
    for &(from, to) in comments {
        let (from, to) = (from.min(chars.len()), to.min(chars.len()));
        let text: String = chars[from..to].iter().collect();
        // A comment of a kind is coloured whole.
        let start = text[..after_marker(&text)].chars().count();
        let rest: String = text.chars().skip(start).collect();
        if let Some(tag) = kind_of(&rest) {
            found.push((from, to, tag));
            continue;
        }
        // Else its keywords, as whole words in capitals, each to the end of its line of comment if followed by a colon.
        for (word, tag) in KEYWORDS {
            let mut at = 0;
            while let Some(position) = text[at..].find(word) {
                let begin = at + position;
                let end = begin + word.len();
                at = end;
                let before_ok = text[..begin].chars().last().is_none_or(|c| !c.is_alphanumeric());
                let after_ok = text[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
                if before_ok && after_ok {
                    let (a, z) = (text[..begin].chars().count(), text[..end].chars().count());
                    found.push((from + a, from + z, tag));
                }
            }
        }
    }
    found.sort();
    found
}

/// Runs of colour (from, to, colour) with stretches given colours of their own laid over them.
pub fn overlay(runs: &[(usize, usize, Color32)], over: &[(usize, usize, Color32)]) -> Vec<(usize, usize, Color32)> {
    let mut out: Vec<(usize, usize, Color32)> = Vec::new();
    for &(a, z, colour) in runs {
        let mut at = a;
        for &(oa, oz, ocolour) in over.iter().filter(|(oa, oz, _)| *oz > a && *oa < z) {
            let (oa, oz) = (oa.max(a), oz.min(z));
            if oa > at {
                out.push((at, oa, colour));
            }
            out.push((oa.max(at), oz, ocolour));
            at = at.max(oz);
        }
        if at < z {
            out.push((at, z, colour));
        }
    }
    out.retain(|(a, z, _)| a < z);
    out
}

/// How far a line is indented, in columns (a tab to the next of `tab`'s stops); None for a blank line.
fn indent_columns(line: &str, tab: usize) -> Option<usize> {
    if line.trim().is_empty() {
        return None;
    }
    let mut columns = 0;
    for c in line.chars() {
        match c {
            ' ' => columns += 1,
            '\t' => columns += tab - columns % tab,
            _ => break,
        }
    }
    Some(columns)
}

/// How many levels of indentation a line has, `step` columns to a level; and for a blank line, none of its own.
pub fn indent_levels(line: &str, tab: usize, step: usize) -> Option<usize> {
    indent_columns(line, tab).map(|columns| columns / step.max(1))
}

/// The step a file indents by: the smallest indent of some of its lines (2 to 8 columns), else a tab's width.
pub fn indent_step<S: AsRef<str>>(lines: impl Iterator<Item = S>, tab: usize) -> usize {
    lines.filter_map(|l| indent_columns(l.as_ref(), tab)).filter(|c| *c > 0).min().filter(|c| (2..=8).contains(c)).unwrap_or(tab.max(1))
}

/// How a colour was written, so a changed one is written back the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Hex3,
    Hex6,
    Hex8,
    Rgb,
    Rgba,
    Hsl,
    /// The Amiga's $0RGB, a nybble for each.
    Amiga,
}

/// A colour written in some text: where it is (by character), what it is, and how it was written.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Written {
    pub from: usize,
    pub to: usize,
    pub colour: Color32,
    pub form: Form,
}

/// The colours written in a line. Amiga $0RGB values count only on a line that names a colour register (COLOR00
/// to COLOR31), where they can't be anything else.
pub fn colours_in(line: &str) -> Vec<Written> {
    let chars: Vec<char> = line.chars().collect();
    let mut found = Vec::new();
    let amiga = line.to_ascii_uppercase().contains("COLOR");
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // #rgb, #rrggbb, #rrggbbaa, not part of a longer word.
        if c == '#' && chars.get(i.wrapping_sub(1)).is_none_or(|p| !p.is_alphanumeric()) {
            let digits: String = chars[i + 1..].iter().take_while(|d| d.is_ascii_hexdigit()).collect();
            let follows_word = chars.get(i + 1 + digits.len()).is_some_and(|n| n.is_alphanumeric() || *n == '_');
            let form = match digits.len() {
                3 => Some(Form::Hex3),
                6 => Some(Form::Hex6),
                8 => Some(Form::Hex8),
                _ => None,
            };
            if let Some(form) = form.filter(|_| !follows_word) {
                let n = |s: &str| u8::from_str_radix(s, 16).unwrap_or(0);
                let colour = match form {
                    Form::Hex3 => {
                        let d: Vec<u8> = digits.chars().map(|d| n(&d.to_string()) * 17).collect();
                        Color32::from_rgb(d[0], d[1], d[2])
                    }
                    Form::Hex6 => Color32::from_rgb(n(&digits[0..2]), n(&digits[2..4]), n(&digits[4..6])),
                    _ => Color32::from_rgba_unmultiplied(n(&digits[0..2]), n(&digits[2..4]), n(&digits[4..6]), n(&digits[6..8])),
                };
                found.push(Written { from: i, to: i + 1 + digits.len(), colour, form });
                i += 1 + digits.len();
                continue;
            }
        }
        // rgb(…), rgba(…), hsl(…), hsla(…).
        let rest: String = chars[i..].iter().take(5).collect::<String>().to_ascii_lowercase();
        if chars.get(i.wrapping_sub(1)).is_none_or(|p| !p.is_alphanumeric())
            && (rest.starts_with("rgb(") || rest.starts_with("rgba(") || rest.starts_with("hsl(") || rest.starts_with("hsla("))
            && let Some(close) = chars[i..].iter().position(|c| *c == ')')
        {
            let inside: String = chars[i..i + close].iter().skip_while(|c| **c != '(').skip(1).collect();
            let parts: Vec<f32> = inside.split([',', ' ', '/']).filter(|p| !p.trim().is_empty()).filter_map(|p| p.trim().trim_end_matches(['%', ';']).trim_end_matches("deg").parse::<f32>().ok()).collect();
            if parts.len() >= 3 {
                let alpha = parts.get(3).map_or(255, |a| if *a <= 1.0 { (a * 255.0) as u8 } else { *a as u8 });
                let (colour, form) = if rest.starts_with("hsl") {
                    let (r, g, b) = hsl_to_rgb(parts[0], parts[1] / 100.0, parts[2] / 100.0);
                    (Color32::from_rgba_unmultiplied(r, g, b, alpha), Form::Hsl)
                } else {
                    (Color32::from_rgba_unmultiplied(parts[0] as u8, parts[1] as u8, parts[2] as u8, alpha), if parts.len() > 3 { Form::Rgba } else { Form::Rgb })
                };
                found.push(Written { from: i, to: i + close + 1, colour, form });
                i += close + 1;
                continue;
            }
        }
        // The Amiga's $0RGB, on a line naming a colour register.
        if amiga && c == '$' && chars.get(i + 1) == Some(&'0') {
            let digits: String = chars[i + 1..].iter().take_while(|d| d.is_ascii_hexdigit()).collect();
            if digits.len() == 4 {
                let n = |k: usize| u8::from_str_radix(&digits[k..k + 1], 16).unwrap_or(0) * 17;
                found.push(Written { from: i, to: i + 5, colour: Color32::from_rgb(n(1), n(2), n(3)), form: Form::Amiga });
                i += 5;
                continue;
            }
        }
        i += 1;
    }
    found
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to(r), to(g), to(b))
}

/// A colour written as it was before.
pub fn write(colour: Color32, form: Form) -> String {
    let [r, g, b, a] = colour.to_srgba_unmultiplied();
    match form {
        Form::Hex3 if r % 17 == 0 && g % 17 == 0 && b % 17 == 0 => format!("#{:X}{:X}{:X}", r / 17, g / 17, b / 17),
        Form::Hex3 | Form::Hex6 => format!("#{r:02X}{g:02X}{b:02X}"),
        Form::Hex8 => format!("#{r:02X}{g:02X}{b:02X}{a:02X}"),
        Form::Rgb => format!("rgb({r}, {g}, {b})"),
        Form::Rgba => format!("rgba({r}, {g}, {b}, {:.2})", a as f32 / 255.0),
        Form::Hsl => {
            let (h, s, l) = rgb_to_hsl(r, g, b);
            format!("hsl({h:.0}, {:.0}%, {:.0}%)", s * 100.0, l * 100.0)
        }
        Form::Amiga => format!("$0{:X}{:X}{:X}", (r as u32 * 15 + 127) / 255, (g as u32 * 15 + 127) / 255, (b as u32 * 15 + 127) / 255),
    }
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if (max - min).abs() < f32::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r { (g - b) / d + if g < b { 6.0 } else { 0.0 } } else if max == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
    (h * 60.0, s, l)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_kinds_are_found_in_comments_only() {
        let line = "x = 1; // TODO: tidy this, NOTE too, but not TODOS";
        let comments = [(7, line.len())];
        let kinds = comment_kinds(line, &comments);
        let words: Vec<&str> = kinds.iter().map(|(a, z, _)| &line[*a..*z]).collect();
        assert_eq!(words, ["TODO", "NOTE"]);
        // A comment beginning with ! is a warning, whole.
        let warning = "// ! don't call this twice";
        assert_eq!(comment_kinds(warning, &[(0, warning.len())]), [(0, warning.len(), 1)]);
        assert_eq!(comment_kinds("# ? why here", &[(0, 12)])[0].2, 5);
    }

    #[test]
    fn colours_laid_over_runs_split_them() {
        let red = Color32::RED;
        let runs = [(0, 10, Color32::GRAY), (10, 20, Color32::WHITE)];
        let over = [(5, 12, red)];
        assert_eq!(overlay(&runs, &over), [(0, 5, Color32::GRAY), (5, 10, red), (10, 12, red), (12, 20, Color32::WHITE)]);
    }

    #[test]
    fn indentation_is_counted_in_levels() {
        assert_eq!(indent_levels("        x", 8, 4), Some(2));
        assert_eq!(indent_levels("\tx", 4, 4), Some(1));
        assert_eq!(indent_levels("   ", 4, 4), None, "a blank line has none of its own");
        // The step is the file's own, whatever a tab's width is.
        assert_eq!(indent_step(["def f():", "    x = 1", "        y = 2"].iter(), 8), 4);
        assert_eq!(indent_step(["no indent"].iter(), 8), 8);
    }

    #[test]
    fn colours_are_found_as_written_and_written_back_the_same_way() {
        let found = colours_in("color: #ff8000; background: rgb(0, 128, 255); border: hsl(120, 100%, 50%); not#abc #12345");
        assert_eq!(found.iter().map(|w| w.form).collect::<Vec<_>>(), [Form::Hex6, Form::Rgb, Form::Hsl]);
        assert_eq!(found[0].colour, Color32::from_rgb(255, 128, 0));
        assert_eq!(found[1].colour, Color32::from_rgb(0, 128, 255));
        assert_eq!(found[2].colour, Color32::from_rgb(0, 255, 0));
        assert_eq!(write(Color32::from_rgb(255, 128, 0), Form::Hex6), "#FF8000");
        assert_eq!(write(Color32::from_rgb(255, 0, 0), Form::Hex3), "#F00");
        assert_eq!(write(Color32::from_rgb(0, 255, 0), Form::Hsl), "hsl(120, 100%, 50%)");
        // The Amiga's, only beside a colour register.
        let copper = colours_in("    dc.w COLOR00,$0F80");
        assert_eq!((copper[0].form, copper[0].colour), (Form::Amiga, Color32::from_rgb(255, 136, 0)));
        assert!(colours_in("    move.w #$0F80,d0").is_empty());
        assert_eq!(write(Color32::from_rgb(255, 136, 0), Form::Amiga), "$0F80");
    }
}
