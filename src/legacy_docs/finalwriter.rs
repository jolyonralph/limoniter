//! Final Writer documents, from SoftWood's word processor for the Amiga: an IFF FORM of type SWRT. Its format was
//! never published; this is as the documents themselves show it, with fw2odf's first notes as a start. Each section
//! (SHD1, with its name) has a text body (TBDY), in which a RULE starts each paragraph (its alignment in its tenth
//! byte) and the text comes in runs, each an ATTR (its length, font, size, style and what it is: text, a tab, a date)
//! followed by the CHRS it describes. Fonts (FDTA) are listed last first, and numbered from 1; a font's name says
//! whether it's the bold or italic face, and the ATTR's style byte holds bold and underline set on any font. The
//! left and right master pages (LMST, RMST) come after the sections, with what's printed on every page. Text is in
//! the Amiga's ISO 8859-1.

use super::{Align, Block, Builder, Doc, Style};

pub fn looks_like(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && bytes.starts_with(b"FORM") && &bytes[8..12] == b"SWRT"
}

/// The chunks of an IFF FORM, nested FORMs left as single chunks: each name and what it holds.
pub(super) fn iff_chunks(bytes: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let end = bytes.get(4..8).map_or(0, |n| (u32::from_be_bytes([n[0], n[1], n[2], n[3]]) as usize).saturating_add(8)).min(bytes.len());
    let mut i = 12;
    while i + 8 <= end {
        let name = [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]];
        let len = u32::from_be_bytes([bytes[i + 4], bytes[i + 5], bytes[i + 6], bytes[i + 7]]) as usize;
        let start = i + 8;
        let stop = start.saturating_add(len).min(end);
        out.push((name, &bytes[start..stop]));
        i = stop.saturating_add(len & 1);
    }
    out
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|b| char::from(*b)).collect()
}

pub fn read(bytes: &[u8]) -> Result<Doc, String> {
    let chunks = iff_chunks(bytes);
    let fonts: Vec<String> = chunks.iter().filter(|(name, _)| name == b"FDTA").map(|(_, data)| latin1(data.split(|b| *b == 0).next().unwrap_or(&[]))).collect();
    let mut b = Builder::default();
    let mut master = Builder::default();
    let mut in_master = false;
    let mut sections = 0;
    let mut started = false;
    let mut pictures = 0;
    let mut attr: Option<&[u8]> = None;
    let mut align = Align::Left;
    for (name, data) in &chunks {
        match name {
            b"SHD1" => {
                if sections > 0 && started {
                    b.block(Block::PageBreak);
                }
                sections += 1;
                started = false;
            }
            b"LMST" | b"RMST" => {
                if !in_master && started {
                    b.end_paragraph();
                }
                in_master = true;
                started = false;
            }
            b"RULE" => {
                let target = if in_master { &mut master } else { &mut b };
                if started {
                    target.end_paragraph();
                }
                started = true;
                align = if data.get(9).is_some_and(|a| a & 0x0F == 4) { Align::Centre } else { Align::Left };
                target.align = align;
            }
            b"ATTR" => attr = Some(data),
            b"CHRS" => {
                let target = if in_master { &mut master } else { &mut b };
                if !started {
                    started = true;
                    target.align = align;
                }
                let a = attr.unwrap_or(&[]);
                let font = a.get(4..6).map_or(0, |f| usize::from(u16::from_be_bytes([f[0], f[1]])));
                let face = if font > 0 { fonts.len().checked_sub(font).and_then(|n| fonts.get(n)).map_or("", |f| f.as_str()) } else { "" };
                let style_bits = a.get(9).copied().unwrap_or(0);
                let place = a.get(10).copied().unwrap_or(0);
                target.style = Style {
                    bold: style_bits & 0x10 != 0 || ["Bold", "Black", "Heavy"].iter().any(|w| face.contains(w)),
                    italic: ["Italic", "Oblique"].iter().any(|w| face.contains(w)),
                    underline: style_bits & 0x01 != 0,
                    superscript: place & 0x02 != 0 || a.get(11) == Some(&17),
                    subscript: place & 0x01 != 0,
                    ..Style::default()
                };
                for &c in *data {
                    match c {
                        b'\t' => target.push('\t'),
                        // A line broken by hand within the paragraph.
                        b'\n' | b'\r' => {
                            target.end_paragraph();
                            target.align = align;
                        }
                        0x00..=0x1F | 0x7F..=0x9F => {}
                        c => target.push(char::from(c)),
                    }
                }
                target.style = Style::default();
            }
            b"FORM" => pictures += 1,
            _ => {}
        }
    }
    let mut blocks = Vec::new();
    let master_text: Vec<String> = master
        .finish()
        .into_iter()
        .filter_map(|block| match block {
            Block::Paragraph { runs, .. } => Some(runs.iter().map(|r| r.text.as_str()).collect::<String>().trim().to_string()),
            _ => None,
        })
        .filter(|t| !t.is_empty())
        .collect();
    if !master_text.is_empty() {
        blocks.push(Block::Header(master_text.join(" ")));
    }
    blocks.extend(b.finish());
    let mut notes = Vec::new();
    if !fonts.is_empty() {
        notes.push(format!("Final Writer's fonts ({}) and sizes aren't carried over.", fonts.join(", ")));
    }
    if pictures > 0 {
        notes.push(format!("{pictures} picture(s) and drawn object(s) aren't carried over."));
    }
    Ok(Doc { format: "Final Writer".into(), blocks, notes })
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

    fn rule(align: u8) -> Vec<u8> {
        let mut data = vec![0; 24];
        data[9] = align;
        chunk(b"RULE", &data)
    }

    /// A run: its length, font and style, then its characters.
    fn run(font: u16, style: u8, text: &[u8]) -> Vec<u8> {
        let mut attr = vec![0; 22];
        attr[..4].copy_from_slice(&(text.len() as u32).to_be_bytes());
        attr[4..6].copy_from_slice(&font.to_be_bytes());
        attr[9] = style;
        [chunk(b"ATTR", &attr), chunk(b"CHRS", text)].concat()
    }

    /// Final Writer 97's own fax template, when ..\testmedia\wp is there.
    #[test]
    fn real_final_writer_document() {
        let Ok(bytes) = std::fs::read("../testmedia/wp/finalwriter-fax-cover") else {
            return;
        };
        assert!(looks_like(&bytes));
        let markdown = Target::Markdown.write(&read(&bytes).unwrap());
        assert!(markdown.starts_with("\\[Company Name\\]\\\n\\[Street Address\\]"));
        assert!(markdown.contains("**DATE:**\tSeptember\u{A0}27,\u{A0}1995\t**TIME:**\t9:00\u{A0}AM"));
    }

    #[test]
    fn final_writer_sections_runs_fonts_and_masters() {
        let mut body = b"SWRT".to_vec();
        body.extend(chunk(b"FDTA", b"SoftSans_Italic\0"));
        body.extend(chunk(b"FDTA", b"SoftSans\0"));
        body.extend(chunk(b"SHD1", b"Main"));
        body.extend(chunk(b"TBDY", &[0, 0]));
        body.extend(rule(4));
        body.extend(run(1, 0x10, b"Fax"));
        body.extend(rule(1));
        body.extend(run(0, 0, b"Plain, "));
        body.extend(run(2, 0, b"slanted"));
        body.extend(run(0, 1, b" and\xA0under"));
        body.extend(chunk(b"SHD1", b"Two"));
        body.extend(chunk(b"TBDY", &[0, 0]));
        body.extend(rule(1));
        body.extend(run(0, 0, b"S\xFCd"));
        body.extend(chunk(b"RMST", &[]));
        body.extend(rule(1));
        body.extend(run(0, 0, b"Page"));
        let mut doc = b"FORM".to_vec();
        doc.extend((body.len() as u32).to_be_bytes());
        doc.extend(body);
        assert!(looks_like(&doc));
        let converted = read(&doc).unwrap();
        assert_eq!(Target::Markdown.write(&converted), "<!-- Header: Page -->\n\n<p align=\"center\">**Fax**</p>\n\nPlain, *slanted* <u>and\u{A0}under</u>\n\n---\n\nS\u{FC}d\n");
    }
}
