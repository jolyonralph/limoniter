//! Signum! documents (.SDO), from Application Systems Heidelberg's word processor for the Atari ST, versions 1 and
//! 2. As Xiphoseer's sdo-tool documents the format: "sdoc", then chunks of a four-letter name and a big-endian
//! length. `cset` names the (up to eight) character sets, Signum!'s own bitmap fonts; `tebu`, the text buffer, holds
//! each page's lines from top to bottom, each with how far down from the last it is and flags (a main line, the
//! first line of a paragraph, a line outside the text such as a formula's), and its characters, each a font, a
//! character and how far it is across from the one before. Lines that aren't main lines carry indices: subscripts
//! just under a line, superscripts just over one.
//!
//! There's no space character: a space is a gap wider than the character before it, so the characters' widths in
//! the editor's fonts (from sdo-tool, for the fonts Signum! came with; ANTIKRO's for any other) tell the gaps that
//! are spaces. The other text fonts put their letters where ANTIKRO does, which is mostly where ASCII does.

use super::{Block, Builder, Doc, Style};

/// ANTIKRO's characters, which the other text fonts share; Signum! has no space, and its § is where ASCII's is.
const CHARS: [char; 128] = [
    '\0', '{', '}', '\0', '\0', '\0', '\0', '↓', '\0', '←', '\0', '→', '\0', '↑', '\0', '[', //
    ']', '<', '>', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '\0', '\0', '\0', //
    '§', '!', '"', '#', '$', '%', '&', '\'', '(', ')', '*', '+', ',', '\u{2212}', '.', '/', //
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', ':', ';', '«', '=', '»', '?', //
    'ü', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', //
    'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'ö', 'Ü', 'ä', '^', '_', //
    '`', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', //
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', 'Ö', '|', 'Ä', '-', 'ß',
];

/// ANTIKRO's widths in the editor, in Signum!'s horizontal units.
const ANTIKRO: [u8; 128] = [
    0, 5, 5, 0, 0, 0, 0, 8, 0, 11, 0, 11, 0, 8, 0, 6, 6, 8, 8, 6, 5, 6, 6, 6, 6, 6, 6, 6, 6, 0, 0, 7, //
    8, 4, 6, 7, 9, 8, 9, 3, 5, 5, 7, 7, 4, 7, 4, 8, 8, 5, 7, 7, 8, 7, 7, 7, 8, 7, 4, 4, 7, 7, 7, 7, //
    8, 10, 9, 10, 10, 9, 8, 10, 11, 5, 6, 10, 8, 13, 10, 11, 8, 11, 9, 8, 9, 11, 10, 14, 11, 10, 9, 8, 11, 7, 9, 9, //
    4, 7, 8, 7, 8, 7, 6, 8, 8, 4, 4, 8, 5, 12, 8, 8, 8, 8, 6, 7, 6, 8, 7, 11, 8, 7, 7, 11, 3, 10, 7, 8,
];

/// NORMANDE's widths.
const NORMANDE: [u8; 128] = [
    0, 5, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 6, 8, 8, 7, 5, 7, 7, 6, 7, 7, 6, 7, 7, 0, 0, 0, //
    8, 5, 8, 7, 10, 9, 9, 4, 7, 7, 7, 7, 5, 7, 5, 9, 9, 6, 9, 9, 8, 8, 9, 8, 9, 9, 5, 5, 6, 7, 6, 7, //
    10, 11, 13, 12, 13, 12, 12, 13, 13, 8, 11, 14, 12, 14, 10, 12, 13, 12, 14, 10, 12, 11, 10, 15, 12, 12, 11, 10, 11, 10, 7, 9, //
    4, 10, 10, 10, 10, 10, 8, 10, 10, 6, 7, 10, 6, 14, 10, 10, 10, 10, 9, 8, 7, 10, 9, 14, 10, 9, 9, 12, 4, 11, 7, 10,
];

/// GROTMIKR's widths, a small font.
const GROTMIKR: [u8; 128] = [
    0, 3, 3, 4, 4, 14, 13, 15, 0, 0, 0, 0, 0, 0, 0, 3, 3, 4, 4, 4, 3, 4, 4, 4, 4, 4, 4, 4, 4, 0, 0, 15, //
    4, 2, 3, 4, 4, 4, 5, 2, 3, 3, 4, 4, 2, 4, 2, 4, 4, 3, 4, 4, 4, 4, 4, 4, 4, 4, 2, 2, 3, 4, 3, 4, //
    4, 5, 4, 5, 5, 4, 4, 5, 5, 2, 3, 5, 4, 6, 5, 5, 4, 5, 4, 4, 4, 5, 5, 9, 6, 5, 5, 4, 5, 4, 4, 5, //
    2, 4, 4, 4, 4, 4, 3, 4, 4, 2, 2, 4, 2, 6, 4, 4, 4, 4, 3, 4, 3, 4, 4, 6, 4, 4, 4, 5, 2, 5, 4, 4,
];

/// PINSEL's widths.
const PINSEL: [u8; 128] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
    9, 5, 8, 7, 14, 9, 9, 5, 6, 6, 7, 7, 5, 7, 5, 9, 9, 7, 8, 8, 9, 8, 8, 9, 8, 8, 5, 5, 6, 7, 6, 7, //
    8, 11, 12, 12, 13, 9, 15, 12, 15, 12, 12, 11, 10, 15, 14, 12, 11, 12, 13, 14, 12, 12, 14, 15, 15, 10, 15, 7, 12, 8, 8, 8, //
    5, 8, 7, 6, 7, 7, 7, 8, 7, 5, 6, 8, 6, 11, 7, 7, 9, 8, 6, 6, 6, 8, 8, 11, 7, 8, 7, 12, 4, 11, 7, 8,
];

fn widths(font: &str) -> &'static [u8; 128] {
    match font {
        "NORMANDE" => &NORMANDE,
        "GROTMIKR" => &GROTMIKR,
        "PINSEL" => &PINSEL,
        _ => &ANTIKRO,
    }
}

pub fn looks_like(bytes: &[u8]) -> bool {
    bytes.starts_with(b"sdoc") && bytes.len() >= 12 && bytes[4..8].iter().all(|b| b.is_ascii_alphanumeric())
}

/// The chunks after "sdoc": each name and what it holds, as far as the file goes.
fn chunks(bytes: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut i = 4;
    while i + 8 <= bytes.len() {
        let name = [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]];
        let len = u32::from_be_bytes([bytes[i + 4], bytes[i + 5], bytes[i + 6], bytes[i + 7]]) as usize;
        let start = i + 8;
        let end = start.saturating_add(len).min(bytes.len());
        out.push((name, &bytes[start..end]));
        i = end;
    }
    out
}

fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    bytes.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

/// One character on a line: where it is across, its font, its code, and its look.
#[derive(Clone, Copy)]
struct Char {
    x: u32,
    font: u8,
    code: u8,
    wide: bool,
    style: Style,
}

/// A line's characters: two bytes for a plain one (top bit set, six bits of offset, two of font, seven of
/// character), or four for one with a look of its own, the offset then in the last eleven bits.
fn line_chars(mut data: &[u8]) -> Vec<Char> {
    let mut out = Vec::new();
    let mut x = 0u32;
    while data.len() >= 2 {
        let cmd = u16::from_be_bytes([data[0], data[1]]);
        let value = (cmd & 0x7E00) >> 9;
        let font = ((cmd & 0x0180) >> 7) as u8;
        let code = (cmd & 0x7F) as u8;
        if cmd & 0x8000 != 0 {
            x += u32::from(value);
            out.push(Char { x, font, code, wide: false, style: Style::default() });
            data = &data[2..];
        } else {
            let Some(extra) = be16(data, 2) else {
                break;
            };
            x += u32::from(extra & 0x07FF);
            let style = Style { bold: extra & 0x4000 != 0, italic: extra & 0x2000 != 0, underline: value & 0x20 != 0, superscript: value & 0x02 != 0, ..Style::default() };
            out.push(Char { x, font: font | (((value & 1) as u8) << 2), code, wide: extra & 0x8000 != 0, style });
            data = &data[4..];
        }
    }
    out
}

/// A line of the text buffer, or the start of a new page.
enum Item {
    Page,
    Line { skip: u16, flags: u16, chars: Vec<Char> },
}

const MAIN: u16 = 0x0400;
const PARAGRAPH: u16 = 0x0800;
const OUTSIDE: u16 = 0x1000;
/// How far down an index line can be from its main line, in Signum!'s vertical units.
const INDEX_REACH: u16 = 8;

pub fn read(bytes: &[u8]) -> Result<Doc, String> {
    let chunks = chunks(bytes);
    let slots: Vec<String> = chunks
        .iter()
        .find(|(name, _)| name == b"cset")
        .map(|(_, data)| data.chunks(10).map(|name| name.iter().take_while(|b| **b != 0).map(|b| crate::ansi_art::glyph(crate::charset::known("atari-st"), *b)).collect::<String>()).collect())
        .unwrap_or_default();
    let Some((_, tebu)) = chunks.iter().find(|(name, _)| name == b"tebu") else {
        return Err("This Signum! document has no text in it.".into());
    };
    // The lines: a vertical skip, a length, then the line: its flags, and for a page's start or end its number,
    // else perhaps one more word and then the characters.
    let mut items = Vec::new();
    let mut i = 4;
    while i + 4 <= tebu.len() {
        let skip = be16(tebu, i).unwrap_or(0);
        let len = usize::from(be16(tebu, i + 2).unwrap_or(0));
        let Some(line) = tebu.get(i + 4..i + 4 + len) else {
            break;
        };
        i += 4 + len;
        let Some(flags) = be16(line, 0) else {
            continue;
        };
        if flags & 0x8000 != 0 {
            if flags & 0x4000 != 0 {
                items.push(Item::Page);
            }
            continue;
        }
        let chars = line_chars(line.get(if flags & 0x0001 != 0 { 4 } else { 2 }..).unwrap_or(&[]));
        items.push(Item::Line { skip, flags, chars });
    }
    // Index lines join the main line they belong to: one just under a main line holds its subscripts, one just over
    // the next main line its superscripts.
    for n in 0..items.len() {
        let (skip, index) = match &items[n] {
            Item::Line { skip, flags, chars } if flags & (MAIN | PARAGRAPH | OUTSIDE) == 0 && !chars.is_empty() => (*skip, true),
            _ => (0, false),
        };
        if !index {
            continue;
        }
        let is_main = |item: &Item| matches!(item, Item::Line { flags, .. } if flags & (MAIN | PARAGRAPH) != 0);
        let below = n > 0 && skip <= INDEX_REACH && is_main(&items[n - 1]);
        let above = !below && items.get(n + 1).is_some_and(|next| is_main(next) && matches!(next, Item::Line { skip, .. } if *skip <= INDEX_REACH));
        if !below && !above {
            continue;
        }
        let Item::Line { chars, .. } = &mut items[n] else {
            continue;
        };
        let mut moved = std::mem::take(chars);
        for c in &mut moved {
            c.style.subscript = below;
            c.style.superscript = !below;
        }
        let into = if below { n - 1 } else { n + 1 };
        if let Item::Line { chars, .. } = &mut items[into] {
            chars.extend(moved);
            chars.sort_by_key(|c| c.x);
        }
    }
    let mut b = Builder::default();
    let mut pages = 0usize;
    let mut used = vec![false; slots.len().max(8)];
    // A line goes on from the one before when it's a main line that doesn't start a paragraph.
    let continues = |item: Option<&Item>| matches!(item, Some(Item::Line { flags, chars, .. }) if flags & MAIN != 0 && flags & (PARAGRAPH | OUTSIDE) == 0 && !chars.is_empty());
    // Whether the last line ended in a hyphen where Signum! broke a word.
    let mut broken = false;
    // The style of the last character written, which a space after it keeps if the next character has it too.
    let mut previous = Style::default();
    for (n, item) in items.iter().enumerate() {
        let (flags, chars) = match item {
            Item::Page => {
                if pages > 0 {
                    b.block(Block::PageBreak);
                }
                pages += 1;
                continue;
            }
            Item::Line { flags, chars, .. } => (*flags, chars),
        };
        if chars.is_empty() {
            continue;
        }
        // A paragraph's first line, a line outside the text, or one on its own starts a paragraph.
        let goes_on = continues(Some(item));
        if !goes_on && !b.current().is_empty() {
            b.end_paragraph();
        }
        // The space where the line wrapped comes before its first character.
        let wrapped = goes_on && !broken && !b.current().is_empty() && !b.current().ends_with(' ');
        // A hyphen (Signum!'s short one, not its minus) ending a line the next goes on from is where a word was
        // broken, and goes.
        let next = items[n + 1..].iter().find(|next| !matches!(next, Item::Line { chars, .. } if chars.is_empty()));
        broken = chars.last().is_some_and(|c| c.code == 0x7E) && continues(next);
        let mut last: Option<(u32, u32)> = None;
        for c in &chars[..chars.len() - usize::from(broken)] {
            let font = slots.get(usize::from(c.font)).map_or("", |f| f.as_str());
            if let Some(u) = used.get_mut(usize::from(c.font)) {
                *u = true;
            }
            let width = u32::from(widths(font)[usize::from(c.code)]) * if c.wide { 2 } else { 1 } + u32::from(c.style.bold);
            if last.map_or(wrapped, |(x, w)| c.x > x + w + 2) {
                // A space between two words in the same style is in it too, so italic words stay one run.
                b.style = if previous == c.style { c.style } else { Style::default() };
                b.push(' ');
            }
            last = Some((c.x, width));
            b.style = c.style;
            previous = c.style;
            match CHARS[usize::from(c.code)] {
                '\0' => {}
                ch => b.push(ch),
            }
        }
        b.style = Style::default();
        if !goes_on && flags & MAIN == 0 {
            b.end_paragraph();
        }
    }
    let fonts: Vec<&str> = slots.iter().enumerate().filter(|(n, name)| !name.is_empty() && used.get(*n).copied().unwrap_or(false)).map(|(_, name)| name.as_str()).collect();
    let mut notes = vec![format!("Signum!'s own bitmap fonts ({}) can't be kept; the text is in plain characters.", if fonts.is_empty() { "none named".to_string() } else { fonts.join(", ") })];
    if items.iter().any(|item| matches!(item, Item::Line { flags, chars, .. } if flags & OUTSIDE != 0 && !chars.is_empty())) {
        notes.push("Text placed outside the lines, such as formulas, is given line by line.".into());
    }
    Ok(Doc { format: "Signum!".into(), blocks: b.finish(), notes })
}

#[cfg(test)]
mod tests {
    use super::super::Target;
    use super::*;

    /// A line of characters: two-byte ones for plain characters, four-byte ones for styled ones.
    fn line(skip: u16, flags: u16, chars: &[(u8, u16, u16)]) -> Vec<u8> {
        let mut data = flags.to_be_bytes().to_vec();
        for &(code, offset, style) in chars {
            if style == 0 && offset < 64 {
                data.extend((0x8000 | (offset << 9) | u16::from(code)).to_be_bytes());
            } else {
                data.extend((u16::from(code)).to_be_bytes());
                data.extend((style | offset).to_be_bytes());
            }
        }
        [skip.to_be_bytes().to_vec(), (data.len() as u16).to_be_bytes().to_vec(), data].concat()
    }

    fn word(text: &str, first: u16) -> Vec<(u8, u16, u16)> {
        // Each character as far on from the last as the last is wide.
        let bytes = text.as_bytes();
        bytes.iter().enumerate().map(|(n, c)| (*c, if n == 0 { first } else { u16::from(ANTIKRO[usize::from(bytes[n - 1])]) }, 0)).collect()
    }

    #[test]
    fn signum_lines_spaces_paragraphs_and_hyphens() {
        let mut tebu = vec![0, 0, 0, 3];
        tebu.extend(line(0, 0xC000, &[]));
        // "Das neue" in bold, then a word broken at the line's end with Signum!'s hyphen.
        let mut first: Vec<(u8, u16, u16)> = b"Das".iter().enumerate().map(|(n, c)| (*c, if n == 0 { 0 } else { 9 }, 0x4000)).collect();
        first.extend(word("neue", 20));
        first.extend(word("Wort\x7E", 20));
        tebu.extend(line(10, 0x0C00, &first));
        tebu.extend(line(10, 0x0400, &word("teil", 0)));
        tebu.extend(line(10, 0x0C00, &word("Ende", 0)));
        tebu.extend(line(10, 0xA000, &[]));
        let mut doc = b"sdoc".to_vec();
        for (name, data) in [(b"0001", vec![0; 128]), (b"cset", b"ANTIKRO\0\0\0".to_vec()), (b"tebu", tebu)] {
            doc.extend(name);
            doc.extend((data.len() as u32).to_be_bytes());
            doc.extend(data);
        }
        assert!(looks_like(&doc));
        let converted = read(&doc).unwrap();
        assert_eq!(Target::Markdown.write(&converted), "**Das** neue Wortteil\\\nEnde\n");
        assert_eq!(converted.notes[0], "Signum!'s own bitmap fonts (ANTIKRO) can't be kept; the text is in plain characters.");
    }

    /// A real document from Signum! 2's own disk, when ..\testmedia\wp is there: its indices come in as subscripts.
    #[test]
    fn real_signum_document() {
        let Ok(bytes) = std::fs::read("../testmedia/wp/signum-physik.sdo") else {
            return;
        };
        assert!(looks_like(&bytes));
        let markdown = Target::Markdown.write(&read(&bytes).unwrap());
        assert!(markdown.contains("DAS ELEKTRISCHE FELD\\\nDie elektrostatische Kraft zwischen zwei Körpern mit den Ladungen Q<sub>1</sub> und Q<sub>2</sub> kann als Fernwirkung"));
        assert!(markdown.contains("der *elektrostatisches Feld* genannt wird"));
    }
}
