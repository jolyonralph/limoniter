//! WordPerfect documents, in the two forms before WordPerfect 6.
//!
//! WordPerfect 4 (DOS 4.2, and the Amiga's and Atari ST's 4.1 and their later releases called 5.0, which save the
//! same way) has no header: text in ASCII, a hard return as &0A and a soft one as &0D (standing for the space where
//! the line wrapped), single-byte codes &80 to &BF that switch attributes on and off (&9D and &9C bold, &94 and &95
//! underline, &B2 and &B3 italics), and multi-byte codes &C0 to &FE, each closed by its own byte again: of a fixed
//! length for most, or running on to the closing byte for the variable ones (headers and footers, footnotes).
//!
//! WordPerfect 5 (DOS 5.0 and 5.1) starts &FF "WPC" with a pointer to the text, and keeps the same idea with
//! different codes: &C0 to &CF of fixed lengths (attributes on and off, tabs, characters from WordPerfect's own
//! character sets), and &D0 to &FF with a length and subgroup at both ends (headers, footers, notes, formatting).
//!
//! As WordPerfect Corporation's developer notes give them, and as libwpd reads them; the extended characters of
//! WordPerfect 4 are the PC's, code page 437, on the Amiga and ST as on DOS.

use super::{Align, Block, Builder, Doc};

/// The lengths of WordPerfect 4's codes &C0 to &FE, both ends included; 0 for those that run to their closing byte.
const WP4_GROUP: [usize; 63] = [
    6, 4, 3, 5, 5, 6, 4, 6, 8, 42, 3, 6, 4, 3, 4, 3, // &C0
    6, 0, 0, 4, 4, 4, 6, 0, 4, 4, 4, 4, 0, 24, 4, 0, // &D0
    4, 3, 0, 150, 6, 23, 11, 3, 3, 0, 0, 0, 4, 0, 44, 18, // &E0
    6, 106, 0, 100, 4, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, // &F0
];

/// The lengths of WordPerfect 5's fixed codes &C0 to &CF.
const WP5_FIXED: [usize; 16] = [4, 9, 11, 3, 3, 5, 6, 7, 4, 5, 6, 6, 8, 10, 10, 12];

/// The header of a WordPerfect 5 document: where its text starts, its minor version, and whether it's locked.
fn wp5_header(bytes: &[u8]) -> Option<(usize, u8, bool)> {
    if bytes.len() < 16 || !bytes.starts_with(b"\xFFWPC") || bytes[9] != 0x0A || bytes[10] != 0 {
        return None;
    }
    let start = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    (16..=bytes.len()).contains(&start).then_some((start, bytes[11], u16::from_le_bytes([bytes[12], bytes[13]]) != 0))
}

/// Whether bytes look like a WordPerfect document: WordPerfect 5's header, or, for WordPerfect 4, which has none,
/// text in which every code is one WordPerfect knows and every multi-byte code is closed where it should be.
pub fn looks_like(bytes: &[u8]) -> bool {
    wp5_header(bytes).is_some() || looks_like_wp4(bytes)
}

/// WordPerfect 4's attribute codes, on and off: bold, underline and italics.
const WP4_PAIRS: [(u8, u8); 3] = [(0x9D, 0x9C), (0x94, 0x95), (0xB2, 0xB3)];

fn looks_like_wp4(bytes: &[u8]) -> bool {
    // A locked WordPerfect 4 document.
    if bytes.starts_with(&[0xFE, 0xFF, 0x61, 0x61]) {
        return bytes.len() >= 6;
    }
    // Text in UTF-8 has bytes that happen to look like codes closed where they should be.
    if bytes.len() < 8 || std::str::from_utf8(bytes).is_ok() {
        return false;
    }
    let (mut text, mut odd, mut fixed, mut high) = (0usize, 0usize, 0usize, 0usize);
    let mut single = [0usize; 64];
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        match byte {
            0x09..=0x0D => {}
            0x00..=0x1F => odd += 1,
            0x20..=0x7F => text += 1,
            0x80..=0xBF => {
                single[usize::from(byte - 0x80)] += 1;
                high += 1;
            }
            0xFF => return false,
            _ => {
                let size = WP4_GROUP[usize::from(byte - 0xC0)];
                if size > 0 {
                    // A fixed code closed by its own byte, with something else inside: a line of &C4s, the PC's
                    // box-drawing line, isn't one, though &E1 &E1 &E1 is the character &E1.
                    let inner = bytes.get(i + 1..i + size - 1);
                    if bytes.get(i + size - 1) != Some(&byte) || inner.is_none_or(|inner| inner.len() > 2 && inner.iter().all(|b| *b == byte)) {
                        return false;
                    }
                    fixed += 1;
                    i += size;
                    continue;
                }
                match bytes[i + 1..].iter().take(0x10000).position(|b| *b == byte) {
                    Some(n) => i += n + 2,
                    None => return false,
                }
                continue;
            }
        }
        i += 1;
    }
    // Attributes are switched on and off again, near enough; letters past ASCII in some machine's character set
    // seldom pair up that way.
    let count = |code: u8| single[usize::from(code - 0x80)];
    if !WP4_PAIRS.iter().all(|(on, off)| count(*on).abs_diff(count(*off)) <= if fixed > 0 { 2 } else { 0 }) {
        return false;
    }
    // With no multi-byte code, attributes are all there is to go on, and nothing else past ASCII but the few
    // single-byte codes plain text has: nothing, centring's end, hard returns and spaces, and hyphens.
    if fixed == 0 {
        let known = |code: u8| WP4_PAIRS.iter().any(|(on, off)| code == *on || code == *off) || matches!(code, 0x80..=0x83 | 0x8C | 0xA0 | 0xA9 | 0xAA | 0xAC | 0xAD);
        if WP4_PAIRS.iter().all(|(on, _)| count(*on) == 0) || (0x80..=0xBF).any(|code| count(code) > 0 && !known(code)) {
            return false;
        }
    }
    text >= 8 && odd * 50 <= text && high * 4 <= text
}

/// What's read as a document goes: the paragraphs, the notes at the foot, and what can't be carried over.
#[derive(Default)]
struct Reading {
    notes: Vec<Block>,
    note_count: usize,
    headers: usize,
    fonts: usize,
    other: usize,
}

impl Reading {
    /// A footnote's or endnote's mark, and the note itself, kept for the end of the document.
    fn note(&mut self, b: &mut Builder, text: String) {
        self.note_count += 1;
        let label = self.note_count.to_string();
        let style = b.style;
        b.style = super::Style { note: true, ..Default::default() };
        for c in label.chars() {
            b.push(c);
        }
        b.style = style;
        self.notes.push(Block::Footnote { label, text });
    }
}

/// The text of a header, footer or note: its paragraphs run together on one line.
fn plain(blocks: Vec<Block>) -> String {
    let lines: Vec<String> = blocks
        .into_iter()
        .filter_map(|block| match block {
            Block::Paragraph { runs, .. } => Some(runs.iter().map(|r| r.text.as_str()).collect::<String>().trim().to_string()),
            _ => None,
        })
        .filter(|line| !line.is_empty())
        .collect();
    lines.join(" ")
}

fn cp437(byte: u8) -> char {
    crate::ansi_art::glyph(crate::charset::known("cp437"), byte)
}

/// WordPerfect 4's text, from a document or from a header, footer or note inside one.
fn wp4_body(bytes: &[u8], b: &mut Builder, r: &mut Reading, depth: usize) {
    // Whether the line so far has nothing but codes in it, so a centring code centres the whole line.
    let mut line_start = true;
    // A hyphen at the end of a line, so the soft return after it isn't a space.
    let mut hyphen_end = false;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        i += 1;
        let was_hyphen = std::mem::take(&mut hyphen_end);
        match byte {
            0x09 => b.push('\t'),
            0x0A | 0x8C => {
                b.end_paragraph();
                line_start = true;
                continue;
            }
            0x0C => {
                b.block(Block::PageBreak);
                line_start = true;
                continue;
            }
            // A soft return or soft page break, where a space was.
            0x0B | 0x0D => {
                if !was_hyphen {
                    b.push(' ');
                }
            }
            0x20..=0x7E => b.push(char::from(byte)),
            0x9D => b.style.bold = true,
            0x9C => b.style.bold = false,
            0x94 => b.style.underline = true,
            0x95 => b.style.underline = false,
            0xB2 => b.style.italic = true,
            0xB3 => b.style.italic = false,
            0x92 => b.style.strike = true,
            0x93 => b.style.strike = false,
            0xA0 => b.push('\u{A0}'),
            0xA9 => b.push('-'),
            0xAA | 0xAB => {
                b.push('-');
                hyphen_end = true;
            }
            // Soft hyphens, shown only where the line broke.
            0xAC => {}
            0xAD | 0xAE => hyphen_end = true,
            // The note's own number, at the start of its text.
            0x8D => {}
            0x80..=0xBF => continue,
            0xC0..=0xFE => {
                let size = WP4_GROUP[usize::from(byte - 0xC0)];
                let end = if size > 0 { (i - 1 + size).min(bytes.len()) } else { bytes[i..].iter().position(|c| *c == byte).map_or(bytes.len(), |n| i + n + 1) };
                let inner = &bytes[i..end.saturating_sub(1).max(i)];
                i = end;
                match byte {
                    // Centred text, to &83; a line that starts with it is a centred line. Centred or flush right
                    // later in a line, it's a tab.
                    0xC3 | 0xC4 if !(line_start && b.current().trim().is_empty()) => b.push('\t'),
                    0xC3 => {
                        b.align = Align::Centre;
                        continue;
                    }
                    0xE1 => {
                        if let Some(&c) = inner.first() {
                            b.push(cp437(c));
                        }
                    }
                    // A header or footer: four bytes, its text to &FF, and which one it is just before the end.
                    0xD1 if depth == 0 => {
                        let text_end = inner.iter().rposition(|c| *c == 0xFF).unwrap_or(inner.len());
                        let which = inner.last().copied().unwrap_or(0) & 3;
                        let mut sub = Builder::default();
                        wp4_body(inner.get(4..text_end).unwrap_or(&[]), &mut sub, r, depth + 1);
                        let text = plain(sub.finish());
                        if !text.is_empty() {
                            let block = if which <= 1 { Block::Header(text) } else { Block::Footer(text) };
                            let (runs, align) = (b.current(), b.align);
                            if runs.trim().is_empty() {
                                b.blocks.push(block);
                                b.align = align;
                            } else {
                                b.block(block);
                            }
                        }
                        r.headers += 1;
                        continue;
                    }
                    // A footnote or endnote: its number and pages to &FF, two bytes of margins, then its text.
                    0xE2 if depth == 0 => {
                        let start = inner.iter().position(|c| *c == 0xFF).map_or(inner.len(), |n| (n + 3).min(inner.len()));
                        let mut sub = Builder::default();
                        wp4_body(&inner[start..], &mut sub, r, depth + 1);
                        let text = plain(sub.finish());
                        r.note(b, text);
                    }
                    // Pitch and font changes.
                    0xCB | 0xD6 => r.fonts += 1,
                    // Indents, which a converted paragraph has no need of.
                    0xCC | 0xE0 | 0xDE | 0xC2 => continue,
                    _ => {
                        r.other += 1;
                        continue;
                    }
                }
            }
            _ => continue,
        }
        line_start = false;
    }
}

/// WordPerfect 5's characters in its own sets, as libwpd's tables give them: the multinational set and typographic
/// symbols, more diacritics, box drawing, iconic symbols, mathematics and Greek. Set 0 is ASCII.
fn wp5_character(set: u8, c: u8) -> char {
    const MULTINATIONAL: &str = "\u{300}\u{B7}\u{303}\u{302}\u{335}\u{338}\u{301}\u{308}\u{304}\u{313}\u{315}\u{2BC}\u{326}\u{315}\u{B0}\u{307}\u{30B}\u{327}\u{328}\u{30C}\u{337}\u{305}\u{306}ßĸjÁáÂâÄäÀàÅåÆæÇçÉéÊêËëÈèÍíÎîÏïÌìÑñÓóÔôÖöÒòÚúÛûÜüÙùŸÿÃãĐđØøÕõÝýÐðÞþĂăĀāĄąĆćČčĈĉĊċĎďĚěĖėĒēĘęǴǵĞğǦǧĢģĜĝĠġĤĥĦħİiĪīĮįĨĩĲĳĴĵĶķĹĺĽľĻļĿŀŁłŃńNŉŇňŅņŐőŌōŒœŔŕŘřŖŗŚśŠšŞşŜŝŤťŢţŦŧŬŭŰűŪūŲųŮůŨũŴŵŶŷŹźŽžŻżŊŋDdLlNnRrSsTtYy";
    const TYPOGRAPHIC: &str = "●○■•*¶§¡¿«»£¥₧ƒªº½¼¢²ⁿ®©¤¾³‛’‘‟”“–—‹›○□†‡™℠℞●◦■▪□▫‒ﬀﬃﬄﬁﬂ…$₣₢₠₤‚„⅓⅔⅛⅜⅝⅞ⓂⓅ€℅℆‰№—¹";
    let table = |table: &[u32]| table.get(usize::from(c)).and_then(|u| char::from_u32(*u));
    let found = match set {
        0 => (0x20..0x7F).contains(&c).then(|| char::from(c)),
        1 => MULTINATIONAL.chars().nth(usize::from(c)),
        2 => table(&INTERNATIONAL),
        3 => table(&BOX),
        4 => TYPOGRAPHIC.chars().nth(usize::from(c)),
        5 => table(&ICONIC),
        6 => table(&MATH),
        8 => table(&GREEK),
        _ => None,
    };
    found.unwrap_or('\u{FFFD}')
}

/// WordPerfect 5's character set 2, more diacritics, as Unicode.
const INTERNATIONAL: [u32; 28] = [
    0x0323, 0x0324, 0x02DA, 0x0325, 0x02BC, 0x032D, 0x2017, 0x005F, 0x0138, 0x032E, 0x033E, 0x2018,
    0x0020, 0x02BD, 0x02DB, 0x0327, 0x0321, 0x0322, 0x030D, 0x2019, 0x0329, 0x0020, 0x0621, 0x02BE,
    0x0306, 0x0310, 0x2032, 0x2034,
];

/// WordPerfect 5's character set 3, box drawing, as Unicode.
const BOX: [u32; 88] = [
    0x2591, 0x2592, 0x2593, 0x2588, 0x258C, 0x2580, 0x2590, 0x2584, 0x2500, 0x2502, 0x250C, 0x2510,
    0x2518, 0x2514, 0x251C, 0x252C, 0x2524, 0x2534, 0x253C, 0x2550, 0x2551, 0x2554, 0x2557, 0x255D,
    0x255A, 0x2560, 0x2566, 0x2563, 0x2569, 0x256C, 0x2552, 0x2555, 0x255B, 0x2558, 0x2553, 0x2556,
    0x255C, 0x2559, 0x255E, 0x2565, 0x2561, 0x2568, 0x255F, 0x2564, 0x2562, 0x2567, 0x256B, 0x256A,
    0x2574, 0x2575, 0x2576, 0x2577, 0x2578, 0x2579, 0x257A, 0x257B, 0x257C, 0x257E, 0x257D, 0x257F,
    0x251F, 0x2522, 0x251E, 0x2521, 0x252E, 0x2532, 0x252D, 0x2531, 0x2527, 0x2526, 0x252A, 0x2529,
    0x2536, 0x253A, 0x2535, 0x2539, 0x2541, 0x2546, 0x253E, 0x2540, 0x2544, 0x254A, 0x253D, 0x2545,
    0x2548, 0x2543, 0x2549, 0x2547,
];

/// WordPerfect 5's character set 5, iconic symbols, as Unicode.
const ICONIC: [u32; 35] = [
    0x2665, 0x2666, 0x2663, 0x2660, 0x2642, 0x2640, 0x263C, 0x263A, 0x263B, 0x266A, 0x266C, 0x25AC,
    0x2302, 0x203C, 0x221A, 0x21A8, 0x2310, 0x2319, 0x25D8, 0x25D9, 0x21B5, 0x261E, 0x261C, 0x2713,
    0x2610, 0x2612, 0x2639, 0x266F, 0x266D, 0x266E, 0x260E, 0x231A, 0x231B, 0x2104, 0x23B5,
];

/// WordPerfect 5's character set 6, mathematics, as Unicode.
const MATH: [u32; 238] = [
    0x2212, 0x00B1, 0x2264, 0x2265, 0x221D, 0x002F, 0x2215, 0x2216, 0x00F7, 0x2223, 0x27E8, 0x27E9,
    0x223C, 0x2248, 0x2261, 0x2208, 0x2229, 0x2225, 0x2211, 0x221E, 0x00AC, 0x2192, 0x2190, 0x2191,
    0x2193, 0x2194, 0x2195, 0x25B8, 0x25C2, 0x25B4, 0x25BE, 0x22C5, 0x00B7, 0x2218, 0x2219, 0x212B,
    0x00B0, 0x00B5, 0x203E, 0x00D7, 0x222B, 0x220F, 0x2213, 0x2207, 0x2202, 0x2032, 0x2033, 0x2192,
    0x212F, 0x2113, 0x210F, 0x2111, 0x211C, 0x2118, 0x21C4, 0x21C6, 0x21D2, 0x21D0, 0x21D1, 0x21D3,
    0x21D4, 0x21D5, 0x2197, 0x2198, 0x2196, 0x2199, 0x222A, 0x2282, 0x2283, 0x2286, 0x2287, 0x220D,
    0x2205, 0x2308, 0x2309, 0x230A, 0x230B, 0x226A, 0x226B, 0x2220, 0x2297, 0x2295, 0x2296, 0x2A38,
    0x2299, 0x2227, 0x2228, 0x22BB, 0x22A4, 0x22A5, 0x2312, 0x22A2, 0x22A3, 0x25A1, 0x25A0, 0x25CA,
    0x25C6, 0x27E6, 0x27E7, 0x2260, 0x2262, 0x2235, 0x2234, 0x2237, 0x222E, 0x2112, 0x212D, 0x2128,
    0x2118, 0x20DD, 0x29CB, 0x25C7, 0x22C6, 0x2034, 0x2210, 0x2243, 0x2245, 0x227A, 0x227C, 0x227B,
    0x227D, 0x2203, 0x2200, 0x22D8, 0x22D9, 0x228E, 0x228A, 0x228B, 0x2293, 0x2294, 0x228F, 0x2291,
    0x22E4, 0x2290, 0x2292, 0x22E5, 0x25B3, 0x25BD, 0x25C3, 0x25B9, 0x22C8, 0x2323, 0x2322, 0x25EF,
    0x219D, 0x21A9, 0x21AA, 0x21A3, 0x21BC, 0x21BD, 0x21C0, 0x21C1, 0x21CC, 0x21CB, 0x21BF, 0x21BE,
    0x21C3, 0x21C2, 0x21C9, 0x21C7, 0x22D3, 0x22D2, 0x22D0, 0x22D1, 0x229A, 0x229B, 0x229D, 0x2127,
    0x2221, 0x2222, 0x25C3, 0x25B9, 0x25B5, 0x25BF, 0x2214, 0x2250, 0x2252, 0x2253, 0x224E, 0x224D,
    0x22A8, 0x2258, 0x226C, 0x0285, 0x2605, 0x226E, 0x2270, 0x226F, 0x2271, 0x2241, 0x2244, 0x2247,
    0x2249, 0x2280, 0x22E0, 0x2281, 0x22E1, 0x2284, 0x2285, 0x2288, 0x2289, 0x0020, 0x0020, 0x22E2,
    0x22E3, 0x2226, 0x2224, 0x226D, 0x2204, 0x2209, 0x2247, 0x2130, 0x2131, 0x2102, 0x0020, 0x2115,
    0x211D, 0x225F, 0x22BE, 0x220B, 0x22EF, 0x2026, 0x22EE, 0x22F1, 0x0020, 0x20E1, 0x002B, 0x002D,
    0x003D, 0x002A, 0x2032, 0x2033, 0x2034, 0x210B, 0x2118, 0x2272, 0x2273, 0x0020,
];

/// WordPerfect 5's character set 8, Greek, as Unicode.
const GREEK: [u32; 210] = [
    0x0391, 0x03B1, 0x0392, 0x03B2, 0x0392, 0x03D0, 0x0393, 0x03B3, 0x0394, 0x03B4, 0x0395, 0x03B5,
    0x0396, 0x03B6, 0x0397, 0x03B7, 0x0398, 0x03B8, 0x0399, 0x03B9, 0x039A, 0x03BA, 0x039B, 0x03BB,
    0x039C, 0x03BC, 0x039D, 0x03BD, 0x039E, 0x03BE, 0x039F, 0x03BF, 0x03A0, 0x03C0, 0x03A1, 0x03C1,
    0x03A3, 0x03C3, 0x03A3, 0x03C2, 0x03A4, 0x03C4, 0x03A5, 0x03C5, 0x03A6, 0x03D5, 0x03A7, 0x03C7,
    0x03A8, 0x03C8, 0x03A9, 0x03C9, 0x03AC, 0x03AD, 0x03AE, 0x03AF, 0x03CA, 0x03CC, 0x03CD, 0x03CB,
    0x03CE, 0x03B5, 0x03D1, 0x03F0, 0x03D6, 0x03F1, 0x03D2, 0x03C6, 0x03C9, 0x037E, 0x0387, 0x0384,
    0x00A8, 0x0385, 0x1FED, 0x1FEF, 0x1FC0, 0x1FBD, 0x1FBF, 0x1FBE, 0x1FCE, 0x1FDE, 0x1FCD, 0x1FDD,
    0x1FCF, 0x1FDF, 0x0384, 0x1FEF, 0x1FC0, 0x1FBD, 0x1FBF, 0x1FCE, 0x1FDE, 0x1FCD, 0x1FDD, 0x1FCF,
    0x1FDF, 0x1F70, 0x1FB6, 0x1FB3, 0x1FB4, 0x1FB7, 0x1F00, 0x1F04, 0x1F02, 0x1F06, 0x1F80, 0x1F84,
    0x1F86, 0x1F01, 0x1F05, 0x1F03, 0x1F07, 0x1F81, 0x1F85, 0x1F87, 0x1F72, 0x1F10, 0x1F14, 0x1F13,
    0x1F11, 0x1F15, 0x1F13, 0x1F74, 0x1FC6, 0x1FC3, 0x1FC4, 0x1FC2, 0x1FC7, 0x1F20, 0x1F24, 0x1F22,
    0x1F26, 0x1F90, 0x1F94, 0x1F96, 0x1F21, 0x1F25, 0x1F23, 0x1F27, 0x1F91, 0x1F95, 0x1F97, 0x1F76,
    0x1FD6, 0x0390, 0x1FD2, 0x1F30, 0x1F34, 0x1F32, 0x1F36, 0x1F31, 0x1F35, 0x1F33, 0x1F37, 0x1F78,
    0x1F40, 0x1F44, 0x1F42, 0x1F41, 0x1F45, 0x1F43, 0x1F7A, 0x1FE6, 0x03B0, 0x1FE3, 0x1F50, 0x1F54,
    0x1F52, 0x1F56, 0x1F51, 0x1F55, 0x1F53, 0x1F57, 0x1F7C, 0x1FF6, 0x1FF3, 0x1FF4, 0x1FF2, 0x1FF7,
    0x1F60, 0x1F64, 0x1F62, 0x1F66, 0x1FA0, 0x1FA4, 0x1FA6, 0x1F61, 0x1F65, 0x1F63, 0x1F67, 0x1FA1,
    0x1FA5, 0x1FA7, 0x0374, 0x0375, 0x03DB, 0x03DD, 0x03D9, 0x03E1, 0x0386, 0x0388, 0x0389, 0x038A,
    0x038C, 0x038E, 0x038F, 0x03AA, 0x03AB, 0x1FE5,
];


/// WordPerfect 5's text, from a document or from a header, footer or note inside one.
fn wp5_body(bytes: &[u8], b: &mut Builder, r: &mut Reading, depth: usize) {
    // Justification set to centred, which lasts until it's set to something else.
    let mut centred = false;
    let mut line_start = true;
    let mut hyphen_end = false;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        i += 1;
        let was_hyphen = std::mem::take(&mut hyphen_end);
        match byte {
            0x09 => b.push('\t'),
            0x0A | 0x8C | 0x90 | 0x99 => {
                b.end_paragraph();
                if centred {
                    b.align = Align::Centre;
                }
                line_start = true;
                continue;
            }
            0x0C => {
                b.block(Block::PageBreak);
                line_start = true;
                continue;
            }
            0x0B | 0x0D | 0x93..=0x95 => {
                if !was_hyphen {
                    b.push(' ');
                }
            }
            0x20..=0x7E => b.push(char::from(byte)),
            0xA0 => b.push('\u{A0}'),
            0xA9 => b.push('-'),
            0xAA | 0xAB => {
                b.push('-');
                hyphen_end = true;
            }
            0xAC => {}
            0xAD | 0xAE => hyphen_end = true,
            0xC0..=0xCF => {
                let size = WP5_FIXED[usize::from(byte - 0xC0)];
                // A code not closed where it should be is taken for damage, and skipped as one byte.
                if bytes.get(i - 1 + size - 1) != Some(&byte) {
                    continue;
                }
                let inner = &bytes[i..i - 1 + size - 1];
                i += size - 1;
                match byte {
                    0xC0 => b.push(wp5_character(inner[1], inner[0])),
                    // A tab, of the kind in its first byte: centred on the margins or flush right at a line's start
                    // say how the line goes.
                    0xC1 => match (inner[0] & 0xE8) >> 3 {
                        // Centred or flush right later in a line, it's a tab.
                        0x1C | 0x18 | 0x0C if !(line_start && b.current().trim().is_empty()) => b.push('\t'),
                        0x1C | 0x18 => {
                            b.align = Align::Centre;
                            continue;
                        }
                        0x0C => continue,
                        _ => b.push('\t'),
                    },
                    0xC2 => b.push('\t'),
                    0xC3 | 0xC4 => {
                        let on = byte == 0xC3;
                        match inner[0] {
                            0x05 => b.style.superscript = on,
                            0x06 => b.style.subscript = on,
                            0x08 => b.style.italic = on,
                            0x0C => b.style.bold = on,
                            0x0D => b.style.strike = on,
                            0x0B | 0x0E => b.style.underline = on,
                            _ => {}
                        }
                        continue;
                    }
                    _ => continue,
                }
            }
            0xD0..=0xFF => {
                // A length and subgroup at each end: the code, the subgroup, the length of the rest; the same length,
                // the subgroup, the code.
                let (Some(&sub), Some(len)) = (bytes.get(i), bytes.get(i + 1..i + 3).map(|l| usize::from(u16::from_le_bytes([l[0], l[1]])))) else {
                    continue;
                };
                let end = i + 3 + len;
                if len < 4 || bytes.get(end - 1) != Some(&byte) || bytes.get(end - 2) != Some(&sub) || bytes.get(end - 4..end - 2) != Some(&bytes[i + 1..i + 3]) {
                    continue;
                }
                let inner = &bytes[i + 3..end - 4];
                i = end;
                match byte {
                    // Justification: centred, or anything else.
                    0xD0 if sub == 0x06 => {
                        if let Some(&how) = inner.get(1) {
                            centred = how == 2;
                            b.align = if centred { Align::Centre } else { Align::Left };
                        }
                        continue;
                    }
                    0xD1 => {
                        r.fonts += 1;
                        continue;
                    }
                    // Headers and footers: A and B of each, and the text after 18 bytes.
                    0xD5 if depth == 0 => {
                        if inner.get(7).is_some_and(|bits| *bits != 0) && inner.len() > 18 {
                            let mut sub_builder = Builder::default();
                            wp5_body(&inner[18..], &mut sub_builder, r, depth + 1);
                            let text = plain(sub_builder.finish());
                            if !text.is_empty() {
                                let block = if sub <= 1 { Block::Header(text) } else { Block::Footer(text) };
                                if b.current().trim().is_empty() {
                                    let align = b.align;
                                    b.blocks.push(block);
                                    b.align = align;
                                } else {
                                    b.block(block);
                                }
                            }
                        }
                        r.headers += 1;
                        continue;
                    }
                    // A footnote or endnote: flags, its number, then for a footnote the pages it runs over.
                    0xD6 if depth == 0 => {
                        let skip = if sub == 0 { inner.get(3).map_or(inner.len(), |pages| 4 + 2 * (usize::from(*pages) + 1) + 9) } else { 7 };
                        let mut sub_builder = Builder::default();
                        wp5_body(inner.get(skip..).unwrap_or(&[]), &mut sub_builder, r, depth + 1);
                        let text = plain(sub_builder.finish());
                        r.note(b, text);
                    }
                    // A table's cells: a new cell is a tab, a new row or the table's end a new line.
                    0xDC | 0xDD => {
                        if byte == 0xDC && sub == 0 {
                            b.push('\t');
                        } else if !b.current().trim().is_empty() {
                            b.end_paragraph();
                            line_start = true;
                            continue;
                        }
                    }
                    _ => {
                        r.other += 1;
                        continue;
                    }
                }
            }
            _ => continue,
        }
        line_start = false;
    }
}

pub fn read(bytes: &[u8]) -> Result<Doc, String> {
    let mut b = Builder::default();
    let mut r = Reading::default();
    let format;
    if let Some((start, minor, locked)) = wp5_header(bytes) {
        if locked {
            return Err("This WordPerfect document is locked with a password.".into());
        }
        format = if minor == 0 { "WordPerfect 5.0" } else { "WordPerfect 5.1" };
        wp5_body(&bytes[start..], &mut b, &mut r, 0);
    } else {
        if bytes.starts_with(&[0xFE, 0xFF, 0x61, 0x61]) {
            return Err("This WordPerfect document is locked with a password.".into());
        }
        format = "WordPerfect 4";
        wp4_body(bytes, &mut b, &mut r, 0);
    }
    let mut blocks = b.finish();
    // A document's last hard returns are blank lines that only end it.
    while matches!(blocks.last(), Some(Block::Paragraph { runs, .. }) if runs.iter().all(|r| r.text.trim().is_empty())) {
        blocks.pop();
    }
    blocks.append(&mut r.notes);
    let mut notes = Vec::new();
    if r.fonts > 0 {
        notes.push(format!("{} font and pitch change(s) aren't carried over.", r.fonts));
    }
    if r.other > 0 {
        notes.push(format!("{} formatting code(s), such as margins, tab settings and spacing, aren't carried over.", r.other));
    }
    Ok(Doc { format: format.into(), blocks, notes })
}

#[cfg(test)]
mod tests {
    use super::super::Target;
    use super::*;

    #[test]
    fn wordperfect_4_styles_centring_soft_returns_and_footnotes() {
        // As the Amiga's WordPerfect 4.1 saves: two bytes of nothing, a centred heading in bold, a paragraph wrapped
        // with a soft return, italics, and a footnote.
        let mut doc = b"\x80\x80\xC3\x00\x2A\x1F\xC3\x9DLone Pine\x9C\x83\x0A\x0A\tThis report is a\x0Dsummary, \xB2truly\xB3 so.".to_vec();
        doc.extend(b"\xE2\x00\x01\x6A\x00\x01\x00\x01\x01\xFF\x0A\x4A\x8DA note.\xE2\x0A");
        assert!(looks_like(&doc));
        let converted = read(&doc).unwrap();
        assert_eq!(converted.format, "WordPerfect 4");
        assert_eq!(Target::Markdown.write(&converted), "<p align=\"center\">**Lone Pine**</p>\n\nThis report is a summary, *truly* so.[^1]\n\n[^1]: A note.\n");
        assert_eq!(Target::Text.write(&converted), "Lone Pine\n\n\tThis report is a summary, truly so.[1]\n[1] A note.\n");
        assert!(Target::Rtf.write(&converted).contains("{\\super 1}{\\footnote\\pard {\\super 1} A note.}"));
    }

    #[test]
    fn wordperfect_5_attributes_extended_characters_and_headers() {
        // Header A on every page: seven bytes, which pages, ten bytes, then its text and two bytes more.
        let header: Vec<u8> = [&[0u8; 7][..], &[1], &[0u8; 10], b"Head", &[0, 0]].concat();
        let len = (header.len() + 4) as u16;
        let mut body = vec![0xD5, 0];
        body.extend(len.to_le_bytes());
        body.extend(&header);
        body.extend(len.to_le_bytes());
        body.extend([0, 0xD5]);
        body.extend(b"Caf\xC0\x29\x01\xC0 \xC3\x0C\xC3bold\xC4\x0C\xC4 and \xC3\x0E\xC3under\xC4\x0E\xC4.\x0A\x0CNext page.\x0A");
        let mut doc = b"\xFFWPC".to_vec();
        doc.extend(16u32.to_le_bytes());
        doc.extend([1, 0x0A, 0, 1, 0, 0, 0, 0]);
        doc.extend(body);
        assert!(looks_like(&doc));
        let converted = read(&doc).unwrap();
        assert_eq!(converted.format, "WordPerfect 5.1");
        assert_eq!(Target::Markdown.write(&converted), "<!-- Header: Head -->\n\nCaf\u{E9} **bold** and <u>under</u>.\n\n---\n\nNext page.\n");
    }

    /// Real documents in ..\testmedia\wp: the Amiga's WordPerfect 4.1, the ST's, DOS 4.2 with footnotes, 5.0 and
    /// a 5.1 table, as each was saved, when they're there.
    #[test]
    fn real_wordperfect_documents() {
        let dir = std::path::Path::new("../testmedia/wp");
        let convert = |name: &str| std::fs::read(dir.join(name)).ok().map(|bytes| (looks_like(&bytes), read(&bytes).unwrap()));
        let Some((true, amiga)) = convert("amiga-wp41-report.lrn") else {
            return;
        };
        let markdown = Target::Markdown.write(&amiga);
        assert!(markdown.starts_with("<p align=\"center\">**Lone Pine National Park**</p>\n\n<p align=\"center\">Annual Report</p>"));
        assert!(markdown.contains("<u>Division I - Big Game Reserve</u>\\\nThis section covers permits issued,"));
        assert!(markdown.contains("once-in-a-lifetime"));
        let (_, st) = convert("st-wp-printer.tst").unwrap();
        assert!(Target::Markdown.write(&st).contains("For instance *Italics* is *Italicized,* **bold** appears **bolded**, <u>underline</u>"));
        let (_, notes) = convert("dos-wp42-table.lrn").unwrap();
        assert!(Target::Markdown.write(&notes).contains("then Secretary John Lewis[^1]"));
        assert!(Target::Markdown.write(&notes).contains("[^1]: On January 13, 1982, Leslie M. Wing"));
        let (_, five) = convert("dos-wp50-past.wkb").unwrap();
        assert_eq!(five.format, "WordPerfect 5.0");
        assert!(Target::Text.write(&five).contains("Howard Keele, No Good Deed Goes Unpunished"));
        let (_, table) = convert("dos-wp51-april93.cal").unwrap();
        assert!(Target::Text.write(&table).contains("SUNDAY\tMONDAY\tTUESDAY"));
    }

    #[test]
    fn locked_documents_and_other_files_are_told_apart() {
        let mut locked = b"\xFFWPC\x10\x00\x00\x00\x01\x0A\x00\x00\x12\x34\x00\x00".to_vec();
        locked.extend(b"secret");
        assert!(read(&locked).is_err());
        assert!(!looks_like(b"Just plain text, nothing more to it than that.\r\n"));
        // Latin-1 text, and a line of the PC's box drawing, aren't WordPerfect's codes.
        assert!(!looks_like(b"Caf\xE9 na\xEFve, the caf\xE9 again."));
        assert!(!looks_like(&[b"Title\r\n".as_slice(), &[0xC4; 40], b"\r\nMore text follows here.\r\n"].concat()));
    }
}
