//! Arnor's Protext (and Prowort, its German edition), on the Amstrad CPC and PCW, and later the Amiga, Atari ST, PC
//! and Amstrad's NC notebooks. As the CPC format notes (from Kevin Thacker's archive, on CPCWiki) give it for the CPC:
//! text with &0D &0A ending a line the writer ended and &0D &8A one Protext wrapped, &90 for spaces it put in to
//! justify, &91 a non-breaking space, &8E a soft hyphen, &80 to &89 and &8B, &8C place and block markers, and &E0 to
//! &FA the printer codes @ and a to z, each turning its effect on and then off (b bold, i italic, u underline, s and
//! t subscript and superscript). Protext's 16- and 32-bit versions keep the top half of the character set for text,
//! so they write each of these codes after an &05; an &05 before anything else stands for that character itself.
//! Lines starting > hold stored commands (>CE centres, >PA starts a page, >HE and >FO set the header and footer, as
//! Protext's manual lists them) or, after > -, a ruler. Files are often padded out with &1A. On the Amiga, a line the
//! writer ended may end with &0A alone.
//!
//! There's nothing in a Protext file to say which machine wrote it. Characters past ASCII are taken in the Atari ST's
//! set (much like the PC's) when most of them fall where its letters are, and in ISO 8859-1, the Amiga's, otherwise.

use super::{Align, Block, Builder, Doc};

/// The printer codes' letters with an effect here.
fn effect(b: &mut Builder, letter: u8) -> bool {
    match letter {
        b'b' => b.style.bold = !b.style.bold,
        b'i' => b.style.italic = !b.style.italic,
        b'u' => b.style.underline = !b.style.underline,
        b's' => b.style.subscript = !b.style.subscript,
        b't' => b.style.superscript = !b.style.superscript,
        _ => return false,
    }
    true
}

/// A CPC file's AMSDOS header, if it has one: 128 bytes whose first 67 add up to the next two.
fn amsdos_header(bytes: &[u8]) -> bool {
    bytes.len() >= 128 && bytes[..67].iter().map(|b| u16::from(*b)).sum::<u16>() == u16::from_le_bytes([bytes[67], bytes[68]]) && bytes[..67].iter().any(|b| *b != 0)
}

fn body(bytes: &[u8]) -> &[u8] {
    let bytes = if amsdos_header(bytes) { &bytes[128..] } else { bytes };
    let end = bytes.windows(2).position(|w| w == [0x1A, 0x1A]).or_else(|| (bytes.last() == Some(&0x1A)).then(|| bytes.len() - 1)).unwrap_or(bytes.len());
    &bytes[..end]
}

/// Whether the 16-bit versions' &05 codes are used, rather than the CPC's single bytes.
fn escaped(body: &[u8]) -> bool {
    body.windows(2).filter(|w| w[0] == 0x05 && (w[1] == 0x90 || w[1] == 0x91 || w[1] == 0x8E || (0xE0..=0xFA).contains(&w[1]))).count() >= 1
}

/// Whether bytes look like a Protext document: text with Protext's soft line ends or its codes, and nothing that
/// isn't text.
pub fn looks_like(bytes: &[u8]) -> bool {
    let body = body(bytes);
    if body.len() < 16 || body.contains(&0) {
        return false;
    }
    let soft = body.windows(2).filter(|w| w == &[0x0D, 0x8A]).count();
    let hard = body.windows(2).filter(|w| w == &[0x0D, 0x0A]).count();
    let codes = body.windows(2).filter(|w| w[0] == 0x05 && (w[1] == 0x90 || w[1] == 0x91 || w[1] == 0x8E || (0xE0..=0xFA).contains(&w[1]))).count();
    // Controls other than Protext's own are few: the PC's arrows and card suits, used as characters.
    let controls = body.iter().filter(|b| **b < 0x20 && !matches!(**b, 0x0D | 0x0A | 0x09 | 0x0C | 0x05)).count();
    if controls * 50 > body.len() || hard == 0 && soft == 0 {
        return false;
    }
    // Every &0D starts a line end.
    let bare_returns = body.windows(2).filter(|w| w[0] == 0x0D && w[1] != 0x0A && w[1] != 0x8A).count();
    if bare_returns > 0 {
        return false;
    }
    if codes >= 2 || (soft >= 2 && codes >= 1) {
        return true;
    }
    // The CPC's own: soft line ends and single-byte codes, with no other top-bit bytes than Protext's.
    let cpc_codes = body.iter().filter(|b| matches!(**b, 0x90 | 0x91 | 0x8E | 0xE0..=0xFA)).count();
    let unknown = body.iter().filter(|b| **b >= 0x80 && !matches!(**b, 0x80..=0x8C | 0x8E | 0x90 | 0x91 | 0xE0..=0xFA)).count();
    soft >= 2 && cpc_codes >= 2 && unknown == 0
}

pub fn read(bytes: &[u8]) -> Doc {
    let body = body(bytes);
    let sixteen = escaped(body);
    // The text's own top-half characters: where most fall says whose set they're in.
    let mut low_half = 0;
    let mut high_half = 0;
    if sixteen {
        let mut i = 0;
        while i < body.len() {
            match body[i] {
                0x05 => i += 1,
                0x0D => i += 1,
                c if (0x80..0xC0).contains(&c) => low_half += 1,
                c if c >= 0xC0 => high_half += 1,
                _ => {}
            }
            i += 1;
        }
    }
    let atari = low_half > high_half;
    let set = crate::charset::known(if atari { "atari-st" } else { "iso-8859-1" });
    let pc = crate::charset::known("cp437");
    let character = |c: u8| -> char {
        if c < 0x20 {
            // The PC's arrows and card suits, which Protext shows for these on every machine.
            crate::ansi_art::glyph(pc, c)
        } else if c < 0x80 {
            char::from(c)
        } else {
            crate::ansi_art::glyph(set, c)
        }
    };

    let mut b = Builder::default();
    let mut commands: Vec<String> = Vec::new();
    let mut rulers = 0;
    let mut printer_codes = 0;
    let mut markers = 0;
    let mut at_line_start = true;
    let mut i = 0;
    while i < body.len() {
        let byte = body[i];
        // A centred line is a stored command with the line's text after it, codes and all.
        if at_line_start && byte == b'>' && body.get(i + 1..i + 3).is_some_and(|c| c.eq_ignore_ascii_case(b"CE")) {
            b.align = Align::Centre;
            i += 3;
            while body.get(i) == Some(&b' ') {
                i += 1;
            }
            at_line_start = false;
            continue;
        }
        if at_line_start && byte == b'>' {
            let end = body[i..].windows(2).position(|w| w[0] == 0x0D).map_or(body.len(), |n| i + n);
            let line: String = body[i + 1..end].iter().map(|c| char::from(*c & 0x7F)).collect();
            stored_command(&mut b, &line, &mut commands, &mut rulers);
            i = (end + 2).min(body.len());
            continue;
        }
        at_line_start = false;
        i += 1;
        // The code a byte or an &05 pair stands for, if it's one, and the character otherwise.
        let code = if sixteen {
            if byte == 0x05 {
                let Some(&next) = body.get(i) else { break };
                i += 1;
                match next {
                    0x80..=0x8C | 0x8E | 0x90 | 0x91 | 0xE0..=0xFA => Some(next),
                    0x09 => Some(0x09),
                    // The club suit, which is &05 itself.
                    0x9F => {
                        b.push('\u{2663}');
                        continue;
                    }
                    c => {
                        b.push(character(c));
                        continue;
                    }
                }
            } else if byte >= 0x80 {
                b.push(character(byte));
                continue;
            } else {
                None
            }
        } else if byte >= 0x80 {
            Some(byte)
        } else {
            None
        };
        match (code, byte) {
            (Some(0x90), _) => {
                // Spaces put in to justify, beside a real one; where there's none, one stands for it.
                if !b.current().ends_with(' ') && body.get(i) != Some(&b' ') && !b.current().is_empty() {
                    b.push(' ');
                }
            }
            (Some(0x91), _) => b.push('\u{A0}'),
            (Some(0x8E), _) => {}
            (Some(0x09), _) => b.push('\t'),
            (Some(c @ 0xE0..=0xFA), _) => {
                if !effect(&mut b, c - 0x80) {
                    printer_codes += 1;
                }
            }
            (Some(_), _) => markers += 1,
            (None, 0x0D) => {
                match body.get(i) {
                    // Wrapped by Protext: the paragraph goes on, its space kept before the line end. Tabs after it
                    // only indent the next line.
                    Some(0x8A) => {
                        i += 1;
                        while body.get(i) == Some(&0x09) || (sixteen && body.get(i..i + 2) == Some(&[0x05, 0x09])) {
                            i += if body[i] == 0x05 { 2 } else { 1 };
                        }
                    }
                    Some(0x0A) => {
                        i += 1;
                        b.end_paragraph();
                    }
                    _ => b.end_paragraph(),
                }
                at_line_start = true;
            }
            // The Amiga's Protext ends lines the writer ended with a line feed alone.
            (None, 0x0A) => {
                b.end_paragraph();
                at_line_start = true;
            }
            (None, 0x09) => b.push('\t'),
            (None, 0x0C) => b.block(Block::PageBreak),
            (None, c) => b.push(character(c)),
        }
    }
    let blocks = b.finish();
    let mut notes = Vec::new();
    if atari {
        notes.push("Read in the Atari ST's character set, which its accented letters suggest.".into());
    }
    if !commands.is_empty() {
        notes.push(format!("Protext's stored commands for {} aren't carried over.", commands.join(", ")));
    }
    if rulers > 0 {
        notes.push(format!("{rulers} ruler line(s), setting margins and tabs, aren't carried over."));
    }
    if printer_codes > 0 {
        notes.push(format!("{printer_codes} printer code(s), such as pitch and font changes, aren't carried over."));
    }
    if markers > 0 {
        notes.push(format!("{markers} place and block marker(s) were left out."));
    }
    Doc { format: if sixteen { "Protext".into() } else { "Protext (CPC)".into() }, blocks, notes }
}

/// A line starting >: a stored command, a comment, or a ruler.
fn stored_command(b: &mut Builder, line: &str, commands: &mut Vec<String>, rulers: &mut usize) {
    let name: String = line.trim_start().chars().take(2).collect::<String>().to_uppercase();
    let rest = line.trim_start().get(2..).unwrap_or("").trim();
    // Comments after a semicolon go.
    let rest = rest.split(';').next().unwrap_or("").trim();
    let mut note = |what: &str| {
        if !commands.iter().any(|c| c == what) {
            commands.push(what.into());
        }
    };
    match name.as_str() {
        // A page break; with a number, only if fewer lines than that are left, which is kept as one.
        "PA" => b.block(Block::PageBreak),
        "HE" | "OH" | "EH" if !rest.eq_ignore_ascii_case("on") && !rest.eq_ignore_ascii_case("off") && !rest.is_empty() => b.block(Block::Header(rest.into())),
        "FO" | "OF" | "EF" if !rest.eq_ignore_ascii_case("on") && !rest.eq_ignore_ascii_case("off") && !rest.is_empty() => b.block(Block::Footer(rest.into())),
        "HE" | "OH" | "EH" | "FO" | "OF" | "EF" | "HM" | "FM" | "PN" | "PE" | "PO" => note("headers, footers and page numbers"),
        "CO" | ">>" => {}
        "LM" | "RM" | "SM" | "OM" | "EM" | "PL" | "TM" | "BM" | "LS" | "LF" | "PS" | "PP" | "PW" | "RJ" | "FP" | "FX" | "CP" | "NP" => note("margins, spacing and layout"),
        "PR" | "CC" | "OC" | "MC" | "RC" | "DC" | "CW" | "MM" | "DP" | "DU" | "PC" | "DM" | "SL" => note("printer and language settings"),
        "AV" | "RV" | "RU" | "IF" | "ID" | "IE" | "EI" | "EL" | "EX" | "DF" | "CF" | "CS" | "IN" | "IP" | "IU" | "IB" | "SV" | "SA" | "MF" | "DE" | "FD" | "FE" | "EA" | "EC" => note("mail merging"),
        "FN" | "EN" | "GC" | "GI" => note("footnotes, contents and index"),
        _ if line.starts_with(['-', '!', '.', ' ']) || line.starts_with(|c: char| c.is_ascii_digit()) => *rulers += 1,
        _ => note("other settings"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Kind, Target, detect, to_text};

    #[test]
    fn protext_sixteen_bit_codes_soft_returns_and_commands() {
        let doc = b">CE A \x05\xe2Title\x05\xe2\r\n>---!----------R\r\n>LM 5\r\nThis \x05\x90is \x05\xf5under\x05\xf5 and \r\x8acaf\x82 with \x05\x1a arrows.\r\n>PA\r\nNext page.\r\n\x1a\x1a\x1a";
        assert_eq!(detect("FILE", doc), Some(Kind::Protext));
        let converted = super::read(doc);
        assert_eq!(Target::Markdown.write(&converted), "<p align=\"center\">A **Title**</p>\n\nThis is <u>under</u> and caf\u{E9} with \u{2192} arrows.\n\n---\n\nNext page.\n");
        assert_eq!(converted.notes, ["Read in the Atari ST's character set, which its accented letters suggest.", "Protext's stored commands for margins, spacing and layout aren't carried over.", "1 ruler line(s), setting margins and tabs, aren't carried over."]);
    }

    #[test]
    fn protext_on_the_cpc_and_plain_text_that_isnt() {
        let mut cpc = b"Some \xe2bold\xe2 text \r\x8athat \x90wraps \r\x8aand ends.\r\n".to_vec();
        cpc.extend([0x1A; 20]);
        assert_eq!(detect("DOC", &cpc), Some(Kind::Protext));
        assert_eq!(to_text(&super::read(&cpc)), "Some bold text that wraps and ends.\n");
        assert!(!super::looks_like(b"Plain text\r\nwith lines\r\nand nothing else\r\n"));
        assert!(!super::looks_like(b"Binary\r\x8a\x00\x01\x02\xe2\xe2\r\x8a\r\x8a"));
    }

    /// Real documents in ..\testmedia\wp, when it's there: TUC's Protext example from ST Format's cover disk 62,
    /// and the first of Protext 3.2's tutorials from cover disk 41.
    #[test]
    fn real_protext_documents() {
        let (Ok(example), Ok(tutor)) = (std::fs::read("../testmedia/wp/PROTEXT.EX"), std::fs::read("../testmedia/wp/TUTOR1")) else {
            return;
        };
        assert_eq!(detect("PROTEXT.EX", &example), Some(Kind::Protext));
        let md = Target::Markdown.write(&super::read(&example));
        assert!(md.contains("Here is **bold** and condensed and elite and *italics* and enlarged"), "{md}");
        assert!(md.contains("<sub>subscript</sub> and <sup>superscript</sup> and <u>underline</u>"), "{md}");
        let text = to_text(&super::read(&tutor));
        assert!(text.contains("\nFor the time being, when you reach the bottom of the screen and wish to read more of the tutorial, just press the down cursor key. The cursor keys"), "{text}");
        assert!(text.contains("SHIFT-\u{2192}"));
    }
}
