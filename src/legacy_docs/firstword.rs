//! GST's 1st Word, given away with the Atari ST, and 1st Word Plus, on the ST, the Archimedes and the PC. Close to
//! WordStar's way of working, as worked out from real documents against what the 1st Word Plus manual (Release 2)
//! says of its spaces and styles: each line ends &0D &0A (on the ST; &0A on the Archimedes), and a line 1st Word
//! wrapped ends with a variable space (&1E), which is what a space typed in a paragraph is saved as; &1C are the
//! stretch spaces it adds to justify, &1D an indent space, and an ordinary space a fixed one. &19 is a soft hyphen at a
//! wrapped line's end, &0C a page break, and &0B with a byte after it the mark of where a page fell. &1B and a byte
//! &80 to &BF sets the style: 1 bold, 2 light, 4 italic, 8 underline, &10 superscript, &20 subscript. Lines starting
//! &1F are layout: 0 the page layout, 1 and 2 the header and footer (left, centre and right parts between &1F, # the
//! page number), 9 a ruler, F the fonts, 8 a picture. A file ends at &1A or a zero byte.

use super::{Block, Builder, Doc};

/// Whether bytes look like a 1st Word document: a layout line first, and a ruler near the start.
pub fn looks_like(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(2048)];
    head.len() > 4 && head[0] == 0x1F && matches!(head[1], b'0'..=b'9' | b'F') && head.windows(3).any(|w| w == b"\x1F9[")
}

pub fn read(bytes: &[u8]) -> Doc {
    let end = bytes.iter().position(|c| *c == 0x1A || *c == 0).unwrap_or(bytes.len());
    let body = &bytes[..end];
    // The ST ends lines with CR LF and uses its own character set; the Archimedes ends them with LF alone and uses
    // ISO 8859-1.
    let atari = body.windows(2).any(|w| w == b"\r\n");
    let set = crate::charset::known(if atari { "atari-st" } else { "iso-8859-1" });
    let mut b = Builder::default();
    let mut pictures = 0;
    let mut light = 0;
    let mut layout = false;
    let lines: Vec<&[u8]> = body.split(|c| *c == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l)).collect();
    let last = lines.len().saturating_sub(1);
    let mut continued = false;
    let plain_spaces = !body.contains(&0x1E);
    for (n, line) in lines.iter().enumerate() {
        if line.first() == Some(&0x1F) {
            match line.get(1) {
                Some(b'1' | b'2') => {
                    let parts: Vec<String> = line[2..].split(|c| *c == 0x1F).map(|p| p.iter().map(|c| text_char(set, *c)).collect::<String>().trim().to_string()).filter(|p| !p.is_empty()).collect();
                    if !parts.is_empty() {
                        let text = parts.join(" / ");
                        b.block(if line[1] == b'1' { Block::Header(text) } else { Block::Footer(text) });
                    }
                }
                Some(b'8') => pictures += 1,
                _ => layout = true,
            }
            continued = false;
            continue;
        }
        let mut i = 0;
        // A wrapped paragraph's next line starts with the indent that sets it under the first.
        if continued {
            while matches!(line.get(i), Some(0x1D | 0x1C)) {
                i += 1;
            }
        }
        // The last byte that isn't a stretch space or a style, which says whether the line was wrapped.
        let mut ending = None;
        while i < line.len() {
            let c = line[i];
            i += 1;
            if c != 0x1C && c != 0x1B {
                ending = Some(c);
            }
            match c {
                0x1B => {
                    if let Some(&style) = line.get(i) {
                        i += 1;
                        b.style.bold = style & 1 != 0;
                        b.style.italic = style & 4 != 0;
                        b.style.underline = style & 8 != 0;
                        b.style.superscript = style & 0x10 != 0;
                        b.style.subscript = style & 0x20 != 0;
                        if style & 2 != 0 {
                            light += 1;
                        }
                    }
                }
                // Variable and fixed spaces are spaces; stretch spaces only justify.
                0x1E | b' ' => b.push(' '),
                0x1C => {}
                0x1D | b'\t' => b.push('\t'),
                0x0C => b.block(Block::PageBreak),
                // Where a page fell when it was last laid out, and the line count after it.
                0x0B => i += 1,
                0x19 => {}
                c if c < 0x20 => {}
                c => b.push(text_char(set, c)),
            }
        }
        // A line that ends in a variable space or a soft hyphen was wrapped, and its paragraph goes on. In a document
        // typed with word processing off, which has no variable spaces, so does one ending in a plain space.
        continued = matches!(ending, Some(0x1E | 0x19)) || (plain_spaces && ending == Some(b' ') && line.iter().any(|c| *c > b' '));
        if !continued && n != last {
            b.end_paragraph();
        }
    }
    let mut notes = Vec::new();
    if layout {
        notes.push("Page layout, rulers and fonts aren't carried over.".into());
    }
    if light > 0 {
        notes.push(format!("{light} change(s) to light (grey) text are shown plain."));
    }
    if pictures > 0 {
        notes.push(format!("{pictures} picture(s) aren't carried over."));
    }
    let format = if atari { "1st Word (Atari ST)" } else { "1st Word Plus (Archimedes)" };
    Doc { format: format.into(), blocks: b.finish(), notes }
}

fn text_char(set: &crate::charset::Charset, c: u8) -> char {
    if c < 0x80 {
        if c < 0x20 { ' ' } else { char::from(c) }
    } else {
        crate::ansi_art::glyph(set, c)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Kind, Target, detect, to_text};

    #[test]
    fn first_word_paragraphs_styles_and_headers() {
        let doc = b"\x1f0660103030566\r\n\x1f1\x1fLetter\x1f\r\n\x1f2\x1f#\x1f\r\n\x1f9[....\x7f....]011\r\n\x1b\x81Title\x1b\x80\r\n\r\nThis\x1eis\x1e\x1c\x1c\x1b\x84quite\x1b\x80\x1ea\x1elong\x1e\r\nline\x1ethat\x1ewr\x19\r\naps\x1eand\x1e\x1b\x88ends\x1b\x80.\r\n\x0c\r\n\x1d\x1c\x1c1.\x1d\x1c\x1cCaf\x82\r\n\x1a\x1a";
        assert_eq!(detect("LETTER.DOC", doc), Some(Kind::FirstWord));
        let converted = super::read(doc);
        assert_eq!(to_text(&converted), "Title\n\nThis is quite a long line that wraps and ends.\n\u{C}\n\t1.\tCaf\u{E9}\n");
        assert_eq!(Target::Markdown.write(&converted), "<!-- Header: Letter -->\n\n<!-- Footer: # -->\n\n**Title**\n\nThis is *quite* a long line that wraps and <u>ends</u>.\n\n---\n\n1. Caf\u{E9}\n");
        assert_eq!(converted.format, "1st Word (Atari ST)");
        assert!(!super::looks_like(b"\x1fJust text"));
    }

    /// Real documents in ..\testmedia\wp, when it's there: TUC's 1st Word example from ST Format's cover disk 62,
    /// and the print test from 1st Word Plus's Archimedes disk.
    #[test]
    fn real_first_word_documents() {
        let (Ok(st), Ok(acorn)) = (std::fs::read("../testmedia/wp/1STWORD.EX"), std::fs::read("../testmedia/wp/Printtest")) else {
            return;
        };
        let doc = super::read(&st);
        let md = Target::Markdown.write(&doc);
        assert!(md.contains("Here are the styles :- **bold** <u>underline</u> *italic* light <sup>superscript</sup> <sub>subscript</sub> **<u>bold underline</u>** normal ***italic bold*** etc."), "{md}");
        assert!(md.contains("\n1. Here is a sample paragraph that has been indented so that it wraps around properly onto the next line.\n"), "{md}");
        let doc = super::read(&acorn);
        assert_eq!(doc.format, "1st Word Plus (Archimedes)");
        let md = Target::Markdown.write(&doc);
        assert!(md.contains("**<u>Pica Text</u>**\n\nThis paragraph is printed in Pica text, justified to 65 characters (6.5 inches)\n"), "{md}");
        assert!(doc.notes.iter().any(|n| n.contains("picture")));
    }
}
