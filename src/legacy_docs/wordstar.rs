//! WordStar documents (3.x to 7), and HP's MemoMaker, which saves in the same form. As MicroPro's file format notes
//! and the many descriptions since give it: text with the last letter of each word's top bit set (for justifying),
//! &8D &0A for a line WordStar wrapped and &0D &0A for one the writer ended, &A0 for spaces it put in to justify, &1E
//! and &1F for soft hyphens, control characters that toggle print effects (^B bold, ^S underline, ^Y italic, and
//! more), dot commands at the start of a line (.pa for a new page, .he and .fo for the header and footer), and &1A
//! after the end. WordStar 5 and later add a 128-byte header (starting &1D &7D), extended characters as &1B c &1C,
//! and other embedded data between &1D and &1D.

use super::{Align, Block, Builder, Doc};

/// Whether bytes look like a WordStar document: text with no zero bytes, and either WordStar's soft line breaks or
/// its marked word ends throughout, which plain text never has, or WordStar 5's header.
pub fn looks_like(bytes: &[u8]) -> bool {
    let body = &bytes[..bytes.iter().position(|b| *b == 0x1A).unwrap_or(bytes.len())];
    if bytes.starts_with(&[0x1D, 0x7D]) && bytes.len() >= 128 {
        return true;
    }
    if body.len() < 16 || crate::basic_more::looks_like_bbc(bytes) {
        return false;
    }
    // Every byte has to mean something to WordStar: text (its top bit a word's mark, so with a printable character
    // under it), its soft returns and spaces, and a few print controls. Anything else is some other kind of file.
    let meaningless = body.iter().filter(|b| matches!(**b, 0x00 | 0x7F | 0xFF) || (**b >= 0x80 && **b < 0xA0 && !matches!(**b, 0x8D | 0x8A))).count();
    let controls = body.iter().filter(|b| **b < 0x20 && !matches!(**b, 0x0D | 0x0A | 0x09 | 0x0C)).count();
    if meaningless > 0 || controls * 20 > body.len() {
        return false;
    }
    let soft_breaks = body.windows(2).filter(|w| w == &[0x8D, 0x0A]).count();
    // A marked word end: a letter or punctuation with its top bit set, then a space or line end.
    let marked = body.windows(2).filter(|w| w[0] >= 0xA1 && w[0] <= 0xFE && matches!(w[1], b' ' | 0x0D | 0x8D | 0xA0)).count();
    let words = body.split(|b| matches!(b, b' ' | 0x0D | 0x0A | 0x8D | 0xA0)).filter(|w| !w.is_empty()).count().max(1);
    // Letters past ASCII in a code page are seldom followed by a space as often as one word in five.
    soft_breaks >= 2 || (marked >= 4 && marked * 5 >= words)
}

/// What a dot command is, for the notes: the ones that don't carry over.
fn dot_name(command: &str) -> &'static str {
    match command {
        "mt" | "mb" | "pl" | "lm" | "rm" | "po" | "pm" => "margins and page length",
        "pn" | "op" | "pg" => "page numbering",
        "ls" | "lh" => "line spacing",
        "cw" | "ps" | "pf" | "ul" | "uj" | "oj" | "ig" => "print and justification settings",
        "cp" => "conditional page breaks",
        "df" | "rv" | "av" | "sv" | "fi" | "dm" | "ma" => "merge printing",
        "lq" | "bp" | "sr" | "xq" | "xr" | "xw" | "xx" => "printer settings",
        _ => "other commands",
    }
}

pub fn read(bytes: &[u8]) -> Doc {
    let mut notes: Vec<String> = Vec::new();
    let mut version = "WordStar";
    let mut start = 0;
    if bytes.starts_with(&[0x1D, 0x7D]) && bytes.len() >= 128 {
        start = 128;
        version = "WordStar 5 or later";
    }
    let body = &bytes[start..bytes[start..].iter().position(|b| *b == 0x1A).map_or(bytes.len(), |end| start + end)];
    let mut b = Builder::default();
    let mut dots: Vec<&'static str> = Vec::new();
    let mut printer_codes = 0;
    let mut at_line_start = true;
    let mut i = 0;
    while i < body.len() {
        let byte = body[i];
        // A dot command fills its line, from a dot at its start to the hard return.
        if at_line_start && byte == b'.' {
            let end = body[i..].iter().position(|c| *c == 0x0D || *c == 0x0A).map_or(body.len(), |n| i + n);
            let line: String = body[i + 1..end].iter().map(|c| char::from(c & 0x7F)).collect();
            let command: String = line.chars().take(2).collect::<String>().to_lowercase();
            let rest = line.get(2..).unwrap_or("").trim().to_string();
            match command.as_str() {
                "pa" => b.block(Block::PageBreak),
                "he" | "h1" => b.block(Block::Header(rest)),
                "fo" | "f1" => b.block(Block::Footer(rest)),
                c if c.starts_with('.') || c.starts_with(' ') || c.is_empty() => {}
                c => {
                    let name = dot_name(c);
                    if !dots.contains(&name) {
                        dots.push(name);
                    }
                }
            }
            i = end;
            while i < body.len() && matches!(body[i], 0x0D | 0x0A | 0x8D | 0x8A) {
                i += 1;
            }
            continue;
        }
        at_line_start = false;
        i += 1;
        match byte {
            // A line the writer ended: the end of a paragraph.
            0x0D => {
                if matches!(body.get(i), Some(0x0A | 0x8A)) {
                    i += 1;
                }
                b.end_paragraph();
                at_line_start = true;
            }
            // A line WordStar wrapped: the paragraph goes on. A soft hyphen at its end joins the word back up.
            0x8D => {
                if matches!(body.get(i), Some(0x0A | 0x8A)) {
                    i += 1;
                }
                at_line_start = true;
                // A dot at the start of a wrapped line is text, not a command.
                if body.get(i) == Some(&b'.') {
                    at_line_start = false;
                }
            }
            // Line feeds, plain or with WordStar's top bit (as it writes them in double-spaced text).
            0x0A | 0x8A => {}
            // Soft hyphens, shown only where WordStar broke a word at a line's end.
            0x1E | 0x1F => {}
            // Spaces WordStar put in to justify a line come before a word's real space, and go. Only where no real
            // space follows do they stand for one.
            0xA0 => {
                while body.get(i) == Some(&0xA0) {
                    i += 1;
                }
                if body.get(i) != Some(&b' ') && !b.current().ends_with(' ') && !b.current().is_empty() {
                    b.push(' ');
                }
            }
            0x02 | 0x04 => b.style.bold = !b.style.bold,
            0x13 => b.style.underline = !b.style.underline,
            0x19 => b.style.italic = !b.style.italic,
            0x18 => b.style.strike = !b.style.strike,
            0x14 => b.style.superscript = !b.style.superscript,
            0x16 => b.style.subscript = !b.style.subscript,
            0x09 => b.push('\t'),
            // A non-breaking space.
            0x0F => b.push('\u{A0}'),
            0x0C => b.block(Block::PageBreak),
            // Overprint: the character after it is struck over the one before, which is what's read.
            0x08 => i += 1,
            // WordStar 5's extended characters: &1B, the character, &1C. They're the PC's, code page 437.
            0x1B => {
                if let (Some(&c), Some(&0x1C)) = (body.get(i), body.get(i + 1)) {
                    b.push(crate::ansi_art::glyph(crate::charset::known("cp437"), c));
                    i += 2;
                }
            }
            // WordStar 5's embedded data, from &1D to &1D: fonts, notes and the like.
            0x1D => {
                let end = body[i..].iter().position(|c| *c == 0x1D).map_or(body.len(), |n| i + n + 1);
                printer_codes += 1;
                i = end;
            }
            // Alternate pitch, user print codes, print pauses and phantom characters: printer matters.
            0x01 | 0x03 | 0x05 | 0x06 | 0x07 | 0x0E | 0x11 | 0x12 | 0x17 => printer_codes += 1,
            c if c < 0x20 => printer_codes += 1,
            // Everything else is text, its top bit only WordStar's mark.
            c => b.push(char::from(c & 0x7F)),
        }
    }
    let mut blocks = b.finish();
    // A centred line, in WordStar, is one indented by its own spaces; only a whole document of them would say so.
    for block in &mut blocks {
        if let Block::Paragraph { align, .. } = block {
            *align = Align::Left;
        }
    }
    if !dots.is_empty() {
        notes.push(format!("WordStar's dot commands for {} aren't carried over.", dots.join(", ")));
    }
    if printer_codes > 0 {
        notes.push(format!("{printer_codes} printer code(s), such as pitch changes and pauses, aren't carried over."));
    }
    Doc { format: version.into(), blocks, notes }
}

#[cfg(test)]
mod tests {
    use super::super::{Target, to_text};
    use super::*;

    /// A word with its last letter marked, as WordStar saves it.
    fn marked(word: &str) -> Vec<u8> {
        let mut bytes = word.as_bytes().to_vec();
        if let Some(last) = bytes.last_mut() {
            *last |= 0x80;
        }
        bytes
    }

    #[test]
    fn wordstar_paragraphs_join_wrapped_lines_and_drop_the_marks() {
        let mut doc = b"Dear Dad,\r\n\r\n".to_vec();
        for word in ["As", "I", "said", "on", "the"] {
            doc.extend(marked(word));
            doc.push(b' ');
        }
        doc.extend(b"\x8D\nphone, it was good.\r\n\x1A\x1A\x1A");
        assert!(looks_like(&doc));
        let converted = read(&doc);
        assert_eq!(to_text(&converted), "Dear Dad,\n\nAs I said on the phone, it was good.\n");
        assert!(converted.notes.is_empty());
    }

    #[test]
    fn wordstar_print_effects_dot_commands_and_soft_characters() {
        let doc = b".he Letter to Dad\r\n.mt 3\r\nThis is \x02bold\x02 and \x13under\x13 and \x19it\x19.\r\nA soft hy\x1Fphen and\xA0\xA0 justified\xA0text.\r\n.pa\r\nPage two \x01wide\x0E.\r\n";
        let converted = read(doc);
        assert_eq!(converted.blocks[0], Block::Header("Letter to Dad".into()));
        assert_eq!(Target::Markdown.write(&converted), "<!-- Header: Letter to Dad -->\n\nThis is **bold** and <u>under</u> and *it*.\\\nA soft hyphen and justified text.\n\n---\n\nPage two wide.\n");
        assert_eq!(converted.notes, ["WordStar's dot commands for margins and page length aren't carried over.", "2 printer code(s), such as pitch changes and pauses, aren't carried over."]);
        assert!(Target::Rtf.write(&converted).contains("{\\b bold}"));
    }

    #[test]
    fn plain_text_and_other_files_are_not_taken_for_wordstar() {
        assert!(!looks_like(b"Just a plain text file, nothing special in it at all.\r\n"));
        assert!(!looks_like(&[0u8; 200]));
        assert!(!looks_like("Caf\u{E9} na\u{EF}ve r\u{E9}sum\u{E9} in Latin-1 text with accents here and there".as_bytes()));
        // A tokenised BBC BASIC program has &8D and top-bit bytes too.
        assert!(!looks_like(&[0x0D, 0, 10, 20, 0xE5, 0x8D, 0x54, 0x4A, 0x40, b':', 0xF1, b'"', b'H', b'I', b' ', b'T', b'H', b'E', b'R', b'E', b'"', 0x0D, 0xFF]));
        // Bytes that are nothing to WordStar.
        let mut odd = b"Some words here and more words there, \x8D\n then \x8D\n more".to_vec();
        odd.push(0x80);
        assert!(!looks_like(&odd));
    }
}
