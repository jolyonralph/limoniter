//! Wordworth documents, from Digita's word processor for the Amiga: an IFF FORM of type WOWO. Its format was never
//! published; this is as the documents of Wordworth 1 to 7 show it. Each paragraph is a WTXT chunk of ISO 8859-1
//! text, after a WPAR that sets how the paragraphs from there on look (the alignment in its nineteenth byte, 1 for
//! centred, and the font and style in its twenty-second and twenty-third) and perhaps a WTAB of tab stops. A WFSC
//! after a paragraph lists where in it the font or style changes: twelve bytes each, the place, the font and the
//! style, in the Amiga's own bits (1 underlined, 2 bold, 4 italic). From Wordworth 5, &0F marks where each line
//! wrapped on screen. A WHED or WFOT starts a header or footer, whose paragraphs follow it the same way.

use super::{Align, Block, Builder, Doc, Style};

pub fn looks_like(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && bytes.starts_with(b"FORM") && &bytes[8..12] == b"WOWO"
}

/// The Amiga's style bits as a style.
fn style(bits: u8) -> Style {
    Style { underline: bits & 1 != 0, bold: bits & 2 != 0, italic: bits & 4 != 0, ..Style::default() }
}

/// Where in the text each change of style comes, from a WFSC.
fn changes(data: &[u8]) -> Vec<(usize, Style)> {
    data.as_chunks::<12>().0.iter().map(|c| (u32::from_be_bytes([c[0], c[1], c[2], c[3]]) as usize, style(c[5]))).collect()
}

/// One paragraph's text into a builder, its styles changing where the WFSC says.
fn paragraph(b: &mut Builder, text: &[u8], base: Style, changes: &[(usize, Style)]) {
    b.style = base;
    for (at, &c) in text.iter().enumerate() {
        if let Some((_, s)) = changes.iter().rev().find(|(place, _)| *place == at) {
            b.style = *s;
        }
        match c {
            b'\t' => b.push('\t'),
            0x00..=0x1F | 0x7F..=0x9F => {}
            c => b.push(char::from(c)),
        }
    }
    b.style = Style::default();
}

/// Where the text goes: the document, or the header or footer.
#[derive(Clone, Copy, PartialEq)]
enum Part {
    Body,
    Header,
    Footer,
}

/// The text frames drawn on the pages, each a Wordworth document of its own (FORM WOWO) inside the FORMs of
/// drawn objects, found as far down as they go.
fn frames<'a>(form: &'a [u8], out: &mut Vec<&'a [u8]>, depth: usize) {
    if depth > 8 {
        return;
    }
    let mut i = 12;
    while i + 8 <= form.len() {
        let len = u32::from_be_bytes([form[i + 4], form[i + 5], form[i + 6], form[i + 7]]) as usize;
        let end = (i + 8).saturating_add(len).min(form.len());
        if &form[i..i + 4] == b"FORM" && end >= i + 12 {
            if &form[i + 8..i + 12] == b"WOWO" {
                out.push(&form[i..end]);
            } else {
                frames(&form[i..end], out, depth + 1);
            }
        }
        i = end.saturating_add(len & 1);
    }
}

pub fn read(bytes: &[u8]) -> Result<Doc, String> {
    let mut doc = read_form(bytes);
    // Text frames come after the text, as there's no saying where on the page each one stood.
    let mut inner = Vec::new();
    frames(bytes, &mut inner, 0);
    let mut framed = 0;
    for frame in inner {
        let frame = read_form(frame);
        let text: Vec<Block> = frame.blocks.into_iter().filter(|b| matches!(b, Block::Paragraph { .. })).collect();
        if !text.is_empty() {
            framed += 1;
            doc.blocks.extend(text);
        }
    }
    if framed > 0 {
        doc.notes.push(format!("{framed} text frame(s) are put after the text, in the order they were saved."));
    }
    Ok(doc)
}

fn read_form(bytes: &[u8]) -> Doc {
    let chunks = super::finalwriter::iff_chunks(bytes);
    let mut body = Builder::default();
    let mut header = Builder::default();
    let mut footer = Builder::default();
    let mut part = Part::Body;
    let mut align = Align::Left;
    let mut base = Style::default();
    let mut fonts: Vec<String> = Vec::new();
    let mut pictures = 0;
    // A paragraph is written only when what follows it shows whether a WFSC belongs to it.
    let mut pending: Option<&[u8]> = None;
    let mut i = 0;
    while i < chunks.len() {
        let (name, data) = chunks[i];
        i += 1;
        let target = match part {
            Part::Body => &mut body,
            Part::Header => &mut header,
            Part::Footer => &mut footer,
        };
        if let Some(text) = pending.take() {
            let styled = if &name == b"WFSC" { changes(data) } else { Vec::new() };
            target.align = align;
            paragraph(target, text, base, &styled);
            target.end_paragraph();
            if &name == b"WFSC" {
                continue;
            }
        }
        match &name {
            b"WTXT" => pending = Some(data),
            b"WPAR" => {
                align = if data.get(18) == Some(&1) { Align::Centre } else { Align::Left };
                base = style(data.get(22).copied().unwrap_or(0));
            }
            b"WHED" => part = Part::Header,
            b"WFOT" => part = Part::Footer,
            b"WFNT" => {
                // The font's number and size in four bytes (or, from Wordworth 5, sometimes fourteen), then its name,
                // ended by a zero.
                let end = data.iter().rposition(|b| *b != 0).map_or(0, |n| n + 1).max(4);
                let start = data.get(4..end).and_then(|name| name.iter().rposition(|b| *b == 0)).map_or(4, |n| n + 5);
                let name: String = data[start..end].iter().map(|b| char::from(*b)).collect();
                if !name.is_empty() && !fonts.contains(&name) {
                    fonts.push(name);
                }
            }
            b"WPIC" => pictures += 1,
            _ => {}
        }
    }
    if let Some(text) = pending {
        let target = match part {
            Part::Body => &mut body,
            Part::Header => &mut header,
            Part::Footer => &mut footer,
        };
        target.align = align;
        paragraph(target, text, base, &[]);
        target.end_paragraph();
    }
    let joined = |b: Builder| -> String {
        b.finish()
            .into_iter()
            .filter_map(|block| match block {
                Block::Paragraph { runs, .. } => Some(runs.iter().map(|r| r.text.as_str()).collect::<String>().trim().to_string()),
                _ => None,
            })
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut blocks = Vec::new();
    let (header, footer) = (joined(header), joined(footer));
    if !header.is_empty() {
        blocks.push(Block::Header(header));
    }
    if !footer.is_empty() {
        blocks.push(Block::Footer(footer));
    }
    let mut text = body.finish();
    while matches!(text.last(), Some(Block::Paragraph { runs, .. }) if runs.iter().all(|r| r.text.trim().is_empty())) {
        text.pop();
    }
    blocks.extend(text);
    let mut notes = Vec::new();
    if !fonts.is_empty() {
        notes.push(format!("Wordworth's fonts ({}) and sizes aren't carried over.", fonts.join(", ")));
    }
    if pictures > 0 {
        notes.push(format!("{pictures} picture(s) aren't carried over."));
    }
    Doc { format: "Wordworth".into(), blocks, notes }
}

#[cfg(test)]
mod tests {
    use super::super::Target;
    use super::*;

    fn chunk(name: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = name.to_vec();
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(data);
        if data.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    fn par(align: u8) -> Vec<u8> {
        let mut data = vec![0; 28];
        data[18] = align;
        chunk(b"WPAR", &data)
    }

    /// Wordworth 2's own sample, and a Wordworth 5 document whose text is all in frames, when ..\testmedia\wp is
    /// there.
    #[test]
    fn real_wordworth_documents() {
        let Ok(bytes) = std::fs::read("../testmedia/wp/wordworth2-designers") else {
            return;
        };
        assert!(looks_like(&bytes));
        let markdown = Target::Markdown.write(&read(&bytes).unwrap());
        assert!(markdown.starts_with("<p align=\"center\">The Early Designers of Type</p>\n\n**Tory**\\\n*In 1529 a French artist,"));
        assert!(markdown.contains("**D\u{FC}rer**"));
        let framed = read(&std::fs::read("../testmedia/wp/wordworth5-frames").unwrap()).unwrap();
        assert!(Target::Text.write(&framed).starts_with("schwarz\n80% Grau\nPurpur\n"));
        assert_eq!(framed.notes[1], "24 text frame(s) are put after the text, in the order they were saved.");
    }

    #[test]
    fn wordworth_paragraphs_styles_and_footers() {
        let mut body = b"WOWO".to_vec();
        body.extend(chunk(b"WFNT", b"\x01\x00\x00\x09Pica60\x00"));
        body.extend(par(1));
        body.extend(chunk(b"WTXT", b"The Poem"));
        body.extend(par(0));
        body.extend(chunk(b"WTXT", b"Read The Prelude now.\x0F"));
        // Italic from the fifth character to the sixteenth.
        body.extend(chunk(b"WFSC", &[0, 0, 0, 5, 1, 4, 0, 1, 2, 0, 0, 0, 0, 0, 0, 16, 1, 0, 0, 1, 2, 0, 0, 0]));
        body.extend(chunk(b"WTXT", b"D\xFCrer"));
        body.extend(chunk(b"WFOT", &[3, 1, 0, 0, 0, 0]));
        body.extend(par(0));
        body.extend(chunk(b"WTXT", b"Page foot"));
        let mut doc = b"FORM".to_vec();
        doc.extend((body.len() as u32).to_be_bytes());
        doc.extend(body);
        assert!(looks_like(&doc));
        let converted = read(&doc).unwrap();
        assert_eq!(Target::Markdown.write(&converted), "<!-- Footer: Page foot -->\n\n<p align=\"center\">The Poem</p>\n\nRead *The Prelude* now.\\\nD\u{FC}rer\n");
        assert_eq!(converted.notes, ["Wordworth's fonts (Pica60) and sizes aren't carried over."]);
    }
}
