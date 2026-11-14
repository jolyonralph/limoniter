//! IFF WORD documents, as New Horizons' ProWrite (and their QuickWrite) save them. As New Horizons' FORM WORD
//! description (1987) gives it: FONT chunks number the fonts and sizes used, then a DOC chunk starts the body and HEAD
//! and FOOT the header and footer. In each, a PARA chunk sets the paragraph format (margins, justification, and the
//! first character's font and style) until the next, and each paragraph is a TEXT chunk, followed by an FSCC chunk
//! listing where its font, style or colour changes, if they do. PAGE is a page break; PCTS, PINF and an ILBM BODY are
//! a picture. Styles are the Amiga's own bits (1 underline, 2 bold, 4 italic), the text ISO 8859-1, with &80 standing
//! for the page number. Later ProWrites add page setup chunks (PSET, PREC, SECT), which are left alone.

use super::{Align, Block, Doc, Run, Style};
use std::collections::HashMap;

#[derive(Clone, Copy, Default)]
struct Look {
    font: u8,
    style: u8,
    misc: u8,
}

impl Look {
    fn style(self) -> Style {
        Style {
            underline: self.style & 1 != 0,
            bold: self.style & 2 != 0,
            italic: self.style & 4 != 0,
            superscript: self.misc == 1,
            subscript: self.misc == 2,
            ..Style::default()
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Body,
    Header,
    Footer,
    Picture,
}

/// A paragraph as read, before it's known whether it's a heading.
struct Para {
    runs: Vec<Run>,
    align: Align,
    /// The size of the font most of its characters are in.
    size: u16,
}

pub fn read(bytes: &[u8]) -> Result<Doc, String> {
    let form = super::ftxt::forms(bytes, b"WORD").into_iter().next().ok_or("This IFF file isn't a WORD document.")?;
    let mut sizes: HashMap<u8, u16> = HashMap::new();
    let mut fonts: Vec<String> = Vec::new();
    let mut section = Section::Body;
    let mut para_look = Look::default();
    let mut align = Align::Left;
    let mut right_aligned = 0;
    let mut pictures = 0;
    let mut body: Vec<Option<Para>> = Vec::new();
    let mut header: Vec<String> = Vec::new();
    let mut footer: Vec<String> = Vec::new();
    let mut pending: Option<(&[u8], Vec<(usize, Look)>)> = None;
    let mut size_count: HashMap<u16, usize> = HashMap::new();

    // A paragraph's text and its changes, made into runs once the changes (which follow it) are known.
    let mut finish = |pending: &mut Option<(&[u8], Vec<(usize, Look)>)>, section: Section, look: Look, align: Align, body: &mut Vec<Option<Para>>, header: &mut Vec<String>, footer: &mut Vec<String>, sizes: &HashMap<u8, u16>| {
        let Some((text, changes)) = pending.take() else {
            return;
        };
        let mut runs: Vec<Run> = Vec::new();
        let mut current = look;
        let mut by_size: HashMap<u16, usize> = HashMap::new();
        for (at, byte) in text.iter().enumerate() {
            for (_, change) in changes.iter().filter(|(place, _)| *place == at) {
                current = *change;
            }
            let piece = match byte {
                b'\t' => "\t".to_string(),
                0x80 => "#".into(),
                0x81 => "[date]".into(),
                0x82 => "[time]".into(),
                // The Amiga's fonts draw the soft hyphen as a hyphen, and ProWrite's writers typed two for a dash.
                0xAD => "-".into(),
                c if *c < 0x20 || (0x7F..0xA0).contains(c) => continue,
                c => char::from(*c).to_string(),
            };
            *by_size.entry(sizes.get(&current.font).copied().unwrap_or(0)).or_default() += 1;
            let style = current.style();
            match runs.last_mut() {
                Some(run) if run.style == style => run.text.push_str(&piece),
                _ => runs.push(Run { text: piece, style }),
            }
        }
        let size = by_size.iter().max_by_key(|(size, count)| (**count, **size)).map_or_else(|| sizes.get(&look.font).copied().unwrap_or(0), |(size, _)| *size);
        match section {
            Section::Body => {
                for (size, count) in by_size {
                    *size_count.entry(size).or_default() += count;
                }
                body.push(Some(Para { runs, align, size }));
            }
            Section::Header | Section::Footer => {
                let line: String = runs.iter().map(|r| r.text.as_str()).collect::<String>().trim().to_string();
                if !line.is_empty() {
                    if section == Section::Header { header.push(line) } else { footer.push(line) }
                }
            }
            Section::Picture => {}
        }
    };

    for (id, data) in super::ftxt::chunks(form) {
        if &id != b"FSCC" {
            finish(&mut pending, section, para_look, align, &mut body, &mut header, &mut footer, &sizes);
        }
        match &id {
            b"FONT" if data.len() >= 4 => {
                sizes.insert(data[0], u16::from_be_bytes([data[2], data[3]]));
                let name: String = data[4..].iter().take_while(|c| **c != 0).map(|c| char::from(*c)).collect();
                if !name.is_empty() && !fonts.contains(&name) {
                    fonts.push(name);
                }
            }
            b"DOC " => section = Section::Body,
            b"HEAD" => section = Section::Header,
            b"FOOT" => section = Section::Footer,
            b"PCTS" => section = Section::Picture,
            b"PINF" => pictures += 1,
            b"PARA" if data.len() >= 11 => {
                align = match data[7] {
                    1 => Align::Centre,
                    _ => Align::Left,
                };
                if data[7] == 2 && section == Section::Body {
                    right_aligned += 1;
                }
                para_look = Look { font: data[8], style: data[9], misc: data[10] };
            }
            b"PAGE" if section == Section::Body => body.push(None),
            b"TEXT" => pending = Some((data, Vec::new())),
            b"FSCC" => {
                if let Some((_, changes)) = pending.as_mut() {
                    for change in data.as_chunks::<8>().0 {
                        changes.push((usize::from(u16::from_be_bytes([change[0], change[1]])), Look { font: change[2], style: change[3], misc: change[4] }));
                    }
                }
            }
            _ => {}
        }
    }
    finish(&mut pending, section, para_look, align, &mut body, &mut header, &mut footer, &sizes);
    drop(finish);

    // The body's size is the one most of its text is in; a short paragraph in a bigger one is a heading, the biggest
    // the first level.
    let body_size = size_count.iter().max_by_key(|(size, count)| (**count, **size)).map_or(0, |(size, _)| *size);
    // Three or more such lines in a row, all one size, are something set large (an address, a title page), not
    // headings.
    let mut heading: Vec<bool> = body.iter().map(|p| p.as_ref().is_some_and(|p| is_heading(p, body_size))).collect();
    let mut start = 0;
    while start < body.len() {
        let size = body[start].as_ref().map(|p| p.size);
        let mut end = start;
        while end < body.len() && heading[end] && body[end].as_ref().map(|p| p.size) == size {
            end += 1;
        }
        if end - start >= 3 {
            heading[start..end].iter_mut().for_each(|h| *h = false);
        }
        start = end.max(start + 1);
    }
    let mut bigger: Vec<u16> = body.iter().zip(&heading).filter(|(_, h)| **h).filter_map(|(p, _)| p.as_ref().map(|p| p.size)).collect();
    bigger.sort_unstable_by(|a, b| b.cmp(a));
    bigger.dedup();

    let mut blocks = Vec::new();
    if !header.is_empty() {
        blocks.push(Block::Header(header.join(" / ")));
    }
    if !footer.is_empty() {
        blocks.push(Block::Footer(footer.join(" / ")));
    }
    for (para, heading) in body.into_iter().zip(heading) {
        match para {
            None => blocks.push(Block::PageBreak),
            Some(p) if heading => {
                let level = bigger.iter().position(|s| *s == p.size).unwrap_or(0).min(2) as u8 + 1;
                blocks.push(Block::Heading { level, text: p.runs.iter().map(|r| r.text.as_str()).collect::<String>().trim().to_string() });
            }
            Some(p) => blocks.push(Block::Paragraph { runs: p.runs, align: p.align }),
        }
    }
    let mut notes = Vec::new();
    if !fonts.is_empty() {
        notes.push(format!("Fonts ({}) and sizes aren't carried over, beyond larger text taken for headings.", fonts.join(", ")));
    }
    if right_aligned > 0 {
        notes.push(format!("{right_aligned} right-aligned paragraph format(s) are left-aligned here."));
    }
    if pictures > 0 {
        notes.push(format!("{pictures} picture(s) aren't carried over."));
    }
    notes.push("Margins, indents, tab stops, line spacing and colours aren't carried over.".into());
    Ok(Doc { format: "ProWrite (IFF WORD)".into(), blocks, notes })
}

fn is_heading(p: &Para, body_size: u16) -> bool {
    let len = p.runs.iter().map(|r| r.text.trim().chars().count()).sum::<usize>();
    p.size > body_size && (1..=120).contains(&len)
}

#[cfg(test)]
mod tests {
    use super::super::{Kind, Target, detect};

    fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend((data.len() as u32).to_be_bytes());
        out.extend_from_slice(data);
        if data.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    fn para(justify: u8, font: u8, style: u8) -> Vec<u8> {
        let mut out = vec![1, 0x68, 1, 0x68, 1, 0x68, 0, justify, font, style, 0, 0];
        out.extend([0; 4]);
        chunk(b"PARA", &out)
    }

    #[test]
    fn prowrite_headings_styles_pages_and_headers() {
        let mut body = b"WORD".to_vec();
        body.extend(chunk(b"FONT", b"\x00\x00\x00\x18Helvetica\x00"));
        body.extend(chunk(b"FONT", b"\x01\x00\x00\x0cTimes\x00"));
        body.extend(chunk(b"DOC ", &[0, 1, 0, 0, 0, 0, 0, 0]));
        body.extend(para(1, 0, 0));
        body.extend(chunk(b"TEXT", b"Memo"));
        body.extend(para(0, 1, 0));
        body.extend(chunk(b"TEXT", b"Some bold and underlined text, caf\xe9."));
        body.extend(chunk(b"FSCC", &[0, 5, 1, 2, 0, 0, 0, 0, 0, 9, 1, 0, 0, 0, 0, 0, 0, 14, 1, 1, 0, 0, 0, 0, 0, 24, 1, 0, 0, 0, 0, 0]));
        body.extend(chunk(b"TEXT", b"1.\tFirst"));
        body.extend(chunk(b"TEXT", b"2.\tSecond"));
        body.extend(chunk(b"PAGE", b""));
        body.extend(chunk(b"TEXT", b""));
        body.extend(chunk(b"HEAD", &[1, 0, 0, 0, 0, 0]));
        body.extend(para(1, 1, 0));
        body.extend(chunk(b"TEXT", b"Page \x80"));
        let mut file = b"FORM".to_vec();
        file.extend((body.len() as u32).to_be_bytes());
        file.extend(body);
        assert_eq!(detect("Memo", &file), Some(Kind::ProWrite));
        let doc = super::read(&file).unwrap();
        assert_eq!(Target::Markdown.write(&doc), "<!-- Header: Page # -->\n\n# Memo\n\nSome **bold** and <u>underlined</u> text, caf\u{E9}.\n\n1. First\n2. Second\n\n---\n");
        assert!(doc.notes[0].starts_with("Fonts (Helvetica, Times)"));
    }

    /// Real documents in ..\testmedia\wp, when it's there: ProWrite 1.1's sample memo and ProWrite 3.2's notes for
    /// its file converter.
    #[test]
    fn real_prowrite_documents() {
        let (Ok(memo), Ok(notes)) = (std::fs::read("../testmedia/wp/Memo"), std::fs::read("../testmedia/wp/Convert Notes")) else {
            return;
        };
        let doc = super::read(&memo).unwrap();
        assert!(Target::Markdown.write(&doc).starts_with("# Memo\n\nJanuary 31, 1987\n\nOn March 1, 1987, the national office"));
        assert!(doc.notes.iter().any(|n| n.contains("1 picture")));
        let md = Target::Markdown.write(&super::read(&notes).unwrap());
        assert!(md.starts_with("# ProWrite File Convert\n\n# Version 2.0\n\n**ProWrite File Convert** converts"), "{md}");
        assert!(md.contains("\n1. Double-click on the **Convert** program icon."), "{md}");
        assert!(md.contains("\n- Because Final Copy uses a proprietary font system"), "{md}");
        // An address set large is no run of headings.
        assert!(md.contains("<p align=\"center\">New Horizons Software, Inc.</p>\n\n<p align=\"center\">PO Box 164260</p>"), "{md}");
        assert!(md.contains("\n---\n\n## Limitations of Convert\n"), "{md}");
    }

    /// A ProWrite document in ..\testmedia\wp, when it's there, set in three sizes of heading: the instructions
    /// for Keith Johnson's game Colors.
    #[test]
    fn real_prowrite_headings() {
        let Ok(bytes) = std::fs::read("../testmedia/wp/colorsdoc.prw") else {
            return;
        };
        let md = Target::Markdown.write(&super::read(&bytes).unwrap());
        assert!(md.starts_with("# COLORS V1.0\n\n## By Keith Johnson\n\n### About this file:\n\nThis is the ProWrite version"), "{md}");
    }
}
