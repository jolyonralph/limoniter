//! IFF FTXT, Commodore's and Electronic Arts' standard formatted text: what the Amiga's clipboard holds, what
//! NotePad saves, and what many programs exchange text as. As the FTXT memo (1985, updated 1988) gives it: a FORM FTXT
//! holds CHRS chunks of ISO 8859-1 text, run together, with LF ending each line and ISO 6429 (ANSI X3.64) control
//! sequences for its looks; and FONS chunks naming the fonts those sequences choose by number. A clipboard may hold
//! several FORMs in a CAT. The memo leaves the sequences to the ANSI standard; the ones used are its Select Graphic
//! Rendition (CSI ... m) for bold, italic, underline and font, as the Amiga's console device reads them too.

use super::{Block, Builder, Doc};

/// The IFF chunks in a run of bytes: each one's ID and data, a pad byte after an odd length skipped. A chunk running
/// past the end gets what there is.
pub fn chunks(bytes: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 8 <= bytes.len() {
        let id = [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]];
        let len = u32::from_be_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        let start = at + 8;
        let end = start.saturating_add(len).min(bytes.len());
        out.push((id, &bytes[start..end]));
        at = end + (len & 1);
    }
    out
}

/// The type of an IFF FORM (b"FTXT", b"WORD"...) if the bytes are one, or a CAT or LIST of FORMs all of one type.
pub fn form_type(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.len() < 12 {
        return None;
    }
    match &bytes[..4] {
        b"FORM" => Some(&bytes[8..12]),
        b"CAT " | b"LIST" => {
            let inner = chunks(&bytes[12..]);
            let first = inner.iter().find(|(id, _)| id == b"FORM")?.1.get(..4)?;
            inner.iter().filter(|(id, _)| id == b"FORM").all(|(_, data)| data.get(..4) == Some(first)).then_some(first)
        }
        _ => None,
    }
}

/// The FORMs of the given type in an IFF file, at its top or in a CAT or LIST, each as its chunks after the type.
pub fn forms<'a>(bytes: &'a [u8], kind: &[u8]) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    for (id, data) in chunks(bytes) {
        match &id {
            b"FORM" if data.get(..4) == Some(kind) => out.push(&data[4..]),
            b"CAT " | b"LIST" if data.len() >= 4 => out.extend(forms(&data[4..], kind)),
            _ => {}
        }
    }
    out
}

/// What the reader met that doesn't carry over, counted for the notes.
#[derive(Default)]
struct Dropped {
    fonts: Vec<String>,
    colours: usize,
    other_sequences: usize,
    controls: usize,
}

/// Applies a Select Graphic Rendition's parameters.
fn rendition(b: &mut Builder, params: &str, dropped: &mut Dropped) {
    let numbers: Vec<u32> = if params.is_empty() { vec![0] } else { params.split(';').map(|p| p.trim().parse().unwrap_or(0)).collect() };
    for n in numbers {
        match n {
            0 => {
                b.style.bold = false;
                b.style.italic = false;
                b.style.underline = false;
            }
            1 => b.style.bold = true,
            3 => b.style.italic = true,
            4 => b.style.underline = true,
            22 => b.style.bold = false,
            23 => b.style.italic = false,
            24 => b.style.underline = false,
            // Fonts by number, which FONS chunks name.
            10..=19 => {}
            30..=49 => dropped.colours += 1,
            _ => dropped.other_sequences += 1,
        }
    }
}

pub fn read(bytes: &[u8]) -> Result<Doc, String> {
    let forms = forms(bytes, b"FTXT");
    if forms.is_empty() {
        return Err("This IFF file has no formatted text (FTXT) in it.".into());
    }
    let mut b = Builder::default();
    let mut dropped = Dropped::default();
    let mut own: Vec<[u8; 4]> = Vec::new();
    for (n, form) in forms.iter().enumerate() {
        // Each FORM starts afresh, as IFF has adjacent FORMs independent; a clipboard's several are pieces in turn.
        if n > 0 {
            b.style = Default::default();
            if !b.current().is_empty() {
                b.end_paragraph();
            }
        }
        for (id, data) in chunks(form) {
            match &id {
                b"CHRS" => chars(&mut b, data, &mut dropped),
                b"FONS" => {
                    let name: String = data.get(4..).unwrap_or_default().iter().take_while(|c| **c != 0).map(|c| char::from(*c)).collect();
                    if !name.is_empty() && !dropped.fonts.contains(&name) {
                        dropped.fonts.push(name);
                    }
                }
                // Other programs' own chunks, beside the text.
                id if !own.contains(id) => own.push(*id),
                _ => {}
            }
        }
    }
    let mut blocks = b.finish();
    // The last line's LF leaves nothing after it.
    while matches!(blocks.last(), Some(Block::Paragraph { runs, .. }) if runs.is_empty()) && blocks.len() > 1 {
        blocks.pop();
    }
    let mut notes = Vec::new();
    if !dropped.fonts.is_empty() {
        notes.push(format!("The fonts it names ({}) aren't carried over.", dropped.fonts.join(", ")));
    }
    if dropped.colours > 0 {
        notes.push(format!("{} colour change(s) aren't carried over.", dropped.colours));
    }
    if dropped.other_sequences > 0 {
        notes.push(format!("{} other control sequence(s) aren't carried over.", dropped.other_sequences));
    }
    if !own.is_empty() {
        let names: Vec<String> = own.iter().map(|id| String::from_utf8_lossy(id).trim().to_string()).collect();
        notes.push(format!("Chunks of the program that wrote it ({}) aren't carried over.", names.join(", ")));
    }
    if dropped.controls > 0 {
        notes.push(format!("{} control character(s) with no meaning in FTXT were left out.", dropped.controls));
    }
    Ok(Doc { format: "IFF FTXT".into(), blocks, notes })
}

/// One CHRS chunk's text and control sequences.
fn chars(b: &mut Builder, data: &[u8], dropped: &mut Dropped) {
    let mut i = 0;
    while i < data.len() {
        let c = data[i];
        i += 1;
        match c {
            b'\n' => b.end_paragraph(),
            b'\t' => b.push('\t'),
            0x0C => b.block(Block::PageBreak),
            // CR isn't a line end in FTXT, but text from elsewhere may have it before each LF.
            b'\r' => {}
            // A control sequence: CSI, or ESC [, then parameters and a final character.
            0x9B | 0x1B if c == 0x9B || data.get(i) == Some(&b'[') => {
                if c == 0x1B {
                    i += 1;
                }
                let start = i;
                while i < data.len() && (0x20..=0x3F).contains(&data[i]) {
                    i += 1;
                }
                let params: String = data[start..i].iter().map(|c| char::from(*c)).collect();
                match data.get(i) {
                    Some(b'm') => rendition(b, &params, dropped),
                    // TextCraft's styles: a CSI and one character, &40 plus 1 superscript, 2 subscript, 4 underline,
                    // 8 bold and &10 italic. Without parameters these finals would move the cursor, which text
                    // has no use for.
                    Some(&style @ 0x40..=0x5F) if params.is_empty() => {
                        b.style.superscript = style & 1 != 0;
                        b.style.subscript = style & 2 != 0;
                        b.style.underline = style & 4 != 0;
                        b.style.bold = style & 8 != 0;
                        b.style.italic = style & 0x10 != 0;
                    }
                    Some(0x40..=0x7E) => dropped.other_sequences += 1,
                    // Malformed, a CSI and some other character (as Final Copy's clips have): both go.
                    Some(_) => dropped.other_sequences += 1,
                    None => {}
                }
                i += 1;
            }
            // Other escape sequences: ESC, intermediates, a final character.
            0x1B => {
                while i < data.len() && (0x20..=0x2F).contains(&data[i]) {
                    i += 1;
                }
                i += 1;
                dropped.other_sequences += 1;
            }
            // Device control and its kin, to the string terminator.
            0x90 | 0x9D | 0x9E | 0x9F => {
                i = data[i..].iter().position(|c| *c == 0x9C).map_or(data.len(), |n| i + n + 1);
                dropped.other_sequences += 1;
            }
            // Single shifts to sets never defined: the character is read as it is.
            0x8E | 0x8F => {}
            c if c < 0x20 || (0x7F..0xA0).contains(&c) => dropped.controls += 1,
            // The Amiga's fonts draw the soft hyphen as a hyphen.
            0xAD => b.push('-'),
            // ISO 8859-1, which is the first 256 of Unicode.
            c => b.push(char::from(c)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Target, detect, Kind};
    use super::*;

    fn iff(kind: &[u8; 4], chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut body = kind.to_vec();
        for (id, data) in chunks {
            body.extend_from_slice(*id);
            body.extend((data.len() as u32).to_be_bytes());
            body.extend_from_slice(data);
            if data.len() % 2 == 1 {
                body.push(0);
            }
        }
        let mut out = b"FORM".to_vec();
        out.extend((body.len() as u32).to_be_bytes());
        out.extend(body);
        out
    }

    #[test]
    fn ftxt_text_styles_and_fonts() {
        let fons = b"\x01\x00\x02\x02Times\x00";
        let bytes = iff(b"FTXT", &[(b"FONS", fons), (b"CHRS", b"The \x9b1mquick\x9b0m brown \x1b[3;4mfox\x9b23;24m.\nCaf\xe9\n\x9b10m"), (b"CHRS", b"Four score\x9b32m\n")]);
        assert_eq!(detect("clip", &bytes), Some(Kind::Ftxt));
        let doc = read(&bytes).unwrap();
        assert_eq!(Target::Markdown.write(&doc), "The **quick** brown *<u>fox</u>*.\\\nCaf\u{E9}\\\nFour score\n");
        assert_eq!(doc.notes, ["The fonts it names (Times) aren't carried over.", "1 colour change(s) aren't carried over."]);
    }

    #[test]
    fn a_clipboard_cat_of_forms_and_nonsense() {
        let one = iff(b"FTXT", &[(b"CHRS", b"one\n")]);
        let two = iff(b"FTXT", &[(b"CHRS", b"two")]);
        let mut cat = b"CAT ".to_vec();
        cat.extend(((one.len() + two.len() + 4) as u32).to_be_bytes());
        cat.extend(b"FTXT");
        cat.extend(&one);
        cat.extend(&two);
        assert_eq!(detect("0", &cat), Some(Kind::Ftxt));
        assert_eq!(super::super::to_text(&read(&cat).unwrap()), "one\ntwo\n");
        // A chunk claiming more than there is, and sequences cut short, don't upset it.
        let mut cut = iff(b"FTXT", &[(b"CHRS", b"abc\x9b12;")]);
        cut[16..20].copy_from_slice(&1000u32.to_be_bytes());
        assert!(read(&cut).is_ok());
        assert!(read(&iff(b"ILBM", &[])).is_err());
    }

    /// Real TextCraft Plus documents in ..\testmedia\wp, when it's there: a test of its accents and styles (from
    /// EnigmA Amiga Run 7's cover CD), and a German library's documentation with a superscript in it.
    #[test]
    fn real_textcraft_documents() {
        let (Ok(test), Ok(doc)) = (std::fs::read("../testmedia/wp/TextCfraft.doc"), std::fs::read("../testmedia/wp/Doc.textcraft")) else {
            return;
        };
        let converted = read(&test).unwrap();
        let md = Target::Markdown.write(&converted);
        assert!(md.contains("a \u{E0}\u{E0}\\\ne \u{E8}\u{E8}"), "{md}");
        assert!(md.contains("**Bold**\\\n*italic*\\\n<u>underline</u>"), "{md}");
        assert_eq!(converted.notes, ["Chunks of the program that wrote it (TCPP, TCPH) aren't carried over."]);
        let md = Target::Markdown.write(&read(&doc).unwrap());
        assert!(md.starts_with("MathLibR2 und MathLibR3 sind Mathematik-Bibliotheken f\u{FC}r den R<sup>2</sup> (zweidimensionaler reeller Raum) und R\u{B3}"), "{md}");
    }
}
