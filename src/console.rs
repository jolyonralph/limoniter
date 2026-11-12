//! Reading the Amiga console's control sequences, kept apart from the text's character set.
//! Both forms are recognised, ESC [ and the single byte 0x9B, and whichever the file uses stays as it is.
//! Sequences and their meanings: Amiga ROM Kernel Reference Manual: Devices, console.device.
//! This only reads them. Nothing here changes or removes one, and no screen is emulated.

use crate::charset::{Charset, raw_byte, raw_char};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// The two bytes 0x1B 0x5B.
    EscBracket,
    /// The single byte 0x9B.
    SingleCsi,
}

impl Form {
    pub fn prefix(self) -> &'static [u8] {
        match self {
            Form::EscBracket => &[0x1B, 0x5B],
            Form::SingleCsi => &[0x9B],
        }
    }
}

/// One sequence found in a line, by character position.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sequence {
    pub start: usize,
    pub end: usize,
    pub form: Form,
    pub meaning: String,
}

fn byte_value(c: char) -> Option<u8> {
    raw_byte(c).or_else(|| u8::try_from(c as u32).ok())
}

/// Finds the complete sequences in a line. An unfinished one is left as plain bytes.
pub fn parse_line(line: &str) -> Vec<Sequence> {
    let chars: Vec<char> = line.chars().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (form, body) = match byte_value(chars[i]) {
            Some(0x9B) => (Form::SingleCsi, i + 1),
            Some(0x1B) if chars.get(i + 1).copied().and_then(byte_value) == Some(0x5B) => (Form::EscBracket, i + 2),
            _ => {
                i += 1;
                continue;
            }
        };
        let mut end = body;
        let mut parameters = String::new();
        while let Some(value) = chars.get(end).copied().and_then(byte_value) {
            if (0x20..=0x3F).contains(&value) {
                parameters.push(value as char);
                end += 1;
            } else {
                break;
            }
        }
        match chars.get(end).copied().and_then(byte_value) {
            Some(last) if (0x40..=0x7E).contains(&last) => {
                found.push(Sequence { start: i, end: end + 1, form, meaning: describe(&parameters, last as char) });
                i = end + 1;
            }
            _ => i += 1,
        }
    }
    found
}

fn style_name(code: u32) -> Option<String> {
    Some(match code {
        0 => "plain text".to_string(),
        1 => "bold".to_string(),
        2 => "faint".to_string(),
        3 => "italic".to_string(),
        4 => "underscore".to_string(),
        7 => "reversed".to_string(),
        8 => "concealed".to_string(),
        22 => "normal weight".to_string(),
        23 => "italic off".to_string(),
        24 => "underscore off".to_string(),
        27 => "reversed off".to_string(),
        28 => "concealed off".to_string(),
        30..=37 => format!("text colour {}", code - 30),
        39 => "default text colour".to_string(),
        40..=47 => format!("background colour {}", code - 40),
        49 => "default background".to_string(),
        _ => return None,
    })
}

/// What a sequence does, in words. One not known here is said to be kept as it is.
pub fn describe(parameters: &str, last: char) -> String {
    let first = parameters.split(';').next().and_then(|n| n.parse::<u32>().ok());
    match last {
        'm' => {
            let styles: Vec<String> = parameters
                .split(';')
                .map(|part| part.parse::<u32>().unwrap_or(0))
                .map(|code| style_name(code).unwrap_or_else(|| format!("style {code} (not recognised, kept as it is)")))
                .collect();
            format!("Style: {}", styles.join(", "))
        }
        'A' => format!("Cursor up {}", first.unwrap_or(1)),
        'B' => format!("Cursor down {}", first.unwrap_or(1)),
        'C' => format!("Cursor right {}", first.unwrap_or(1)),
        'D' => format!("Cursor left {}", first.unwrap_or(1)),
        'E' => format!("Cursor to start of line, down {}", first.unwrap_or(1)),
        'F' => format!("Cursor to start of line, up {}", first.unwrap_or(1)),
        'H' => "Cursor to a position".to_string(),
        'J' => "Erase to the end of the display".to_string(),
        'K' => "Erase to the end of the line".to_string(),
        'L' => format!("Insert {} line(s)", first.unwrap_or(1)),
        'M' => format!("Delete {} line(s)", first.unwrap_or(1)),
        '@' => format!("Insert {} character(s)", first.unwrap_or(1)),
        'P' => format!("Delete {} character(s)", first.unwrap_or(1)),
        'S' => format!("Scroll up {}", first.unwrap_or(1)),
        'T' => format!("Scroll down {}", first.unwrap_or(1)),
        'p' if parameters == " " => "Show the cursor".to_string(),
        'p' if parameters == "0 " => "Hide the cursor".to_string(),
        _ => format!("Not recognised: {parameters}{last} is kept as it is"),
    }
}

/// A sequence offered for insertion in the Control sequences panel.
pub struct Template {
    pub group: &'static str,
    pub name: &'static str,
    pub body: &'static str,
}

const fn t(group: &'static str, name: &'static str, body: &'static str) -> Template {
    Template { group, name, body }
}

pub const AMIGA_TEMPLATES: [Template; 34] = [
    t("Style", "Plain text", "0m"),
    t("Style", "Bold", "1m"),
    t("Style", "Faint", "2m"),
    t("Style", "Italic", "3m"),
    t("Style", "Underscore", "4m"),
    t("Style", "Reversed", "7m"),
    t("Style", "Concealed", "8m"),
    t("Style", "Normal weight", "22m"),
    t("Style", "Italic off", "23m"),
    t("Style", "Underscore off", "24m"),
    t("Style", "Reversed off", "27m"),
    t("Text colour", "Colour 0", "30m"),
    t("Text colour", "Colour 1", "31m"),
    t("Text colour", "Colour 2", "32m"),
    t("Text colour", "Colour 3", "33m"),
    t("Text colour", "Colour 4", "34m"),
    t("Text colour", "Colour 5", "35m"),
    t("Text colour", "Colour 6", "36m"),
    t("Text colour", "Colour 7", "37m"),
    t("Text colour", "Default", "39m"),
    t("Background", "Colour 0", "40m"),
    t("Background", "Colour 1", "41m"),
    t("Background", "Colour 2", "42m"),
    t("Background", "Colour 3", "43m"),
    t("Background", "Colour 4", "44m"),
    t("Background", "Colour 5", "45m"),
    t("Background", "Colour 6", "46m"),
    t("Background", "Colour 7", "47m"),
    t("Background", "Default", "49m"),
    t("Cursor", "Show", " p"),
    t("Cursor", "Hide", "0 p"),
    t("Cursor", "Home", "H"),
    t("Erase", "To end of display", "J"),
    t("Erase", "To end of line", "K"),
];

impl Template {
    /// The complete bytes the sequence is made of.
    pub fn bytes(&self, form: Form) -> Vec<u8> {
        let mut bytes = form.prefix().to_vec();
        bytes.extend_from_slice(self.body.as_bytes());
        bytes
    }
}

/// Exact bytes as text for a document in this set.
pub fn bytes_as_text(bytes: &[u8], set: &Charset) -> String {
    bytes
        .iter()
        .map(|byte| {
            if set.holds_characters() {
                if *byte < 0x80 { char::from(*byte) } else { raw_char(*byte) }
            } else {
                set.decoded[usize::from(*byte)]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::charset;

    #[test]
    fn both_forms_are_found_and_kept_apart() {
        let csi = raw_char(0x9B);
        let esc = raw_char(0x1B);
        let found = parse_line(&format!("a{csi}31mb{esc}[1;32mc"));
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].start, found[0].end, found[0].form), (1, 5, Form::SingleCsi));
        assert_eq!(found[0].meaning, "Style: text colour 1");
        assert_eq!((found[1].start, found[1].end, found[1].form), (6, 13, Form::EscBracket));
        assert_eq!(found[1].meaning, "Style: bold, text colour 2");
    }

    #[test]
    fn unfinished_and_unknown_sequences_are_left_alone() {
        assert!(parse_line(&format!("{}31", raw_char(0x9B))).is_empty());
        assert!(parse_line(&format!("{}x", raw_char(0x1B))).is_empty());
        let odd = parse_line(&format!("{}9z", raw_char(0x9B)));
        assert!(odd[0].meaning.contains("kept as it is"));
        let style = parse_line(&format!("{}99m", raw_char(0x9B)));
        assert!(style[0].meaning.contains("not recognised"));
    }

    #[test]
    fn a_unicode_file_can_hold_them_too() {
        assert_eq!(parse_line("\u{1B}[0m").len(), 1);
        assert_eq!(parse_line("\u{9B}0m").len(), 1);
    }

    #[test]
    fn templates_show_their_whole_bytes_in_either_form() {
        let red = &AMIGA_TEMPLATES[12];
        assert_eq!(red.bytes(Form::SingleCsi), [0x9B, b'3', b'1', b'm']);
        assert_eq!(red.bytes(Form::EscBracket), [0x1B, b'[', b'3', b'1', b'm']);
    }

    #[test]
    fn inserted_sequences_save_as_their_exact_bytes() {
        let bytes = AMIGA_TEMPLATES[12].bytes(Form::SingleCsi);
        let amiga = charset::known("amiga-text");
        let text = bytes_as_text(&bytes, amiga);
        assert_eq!(text.chars().next(), Some(raw_char(0x9B)));
        let format = crate::codec::TextFormat { charset: amiga, ..Default::default() };
        let eol = |_: usize| crate::codec::Eol::Lf;
        assert_eq!(crate::codec::encode(&[text], &eol, &format).unwrap(), bytes);
    }
}
