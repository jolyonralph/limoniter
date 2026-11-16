//! PipeDream, the Cambridge Z88's word processor, spreadsheet and database in one, as the Z88 User Guide's
//! Appendix E ("PipeDream file format") describes its files: plain text, with what isn't the text in constructs
//! between percent signs. Options come first (`%OP%` and two letters, one to a line: DE the title, HE the header, FO
//! the footer, and the page's settings), then each column in turn (`%CO:A,12,72%`, its letter, width and wrap
//! width) with one line for each of its slots. A slot's own constructs say how it's aligned and shown, `%V%` makes it
//! a number (its text then being its expression), `%PC%` is a percent sign, `%Pn%` a page break, and `%Hn%` turns a
//! highlight on or off (1 underline, 2 bold, 4 italic, 5 subscript, 6 superscript, as the Z88's printer driver has
//! them out of the box).
//!
//! A document of one column is its paragraphs; one of several is a table, a row for each slot, a column for each
//! column.

use super::{Align, Block, Builder, Doc, Run, Style};

/// Whether bytes are a PipeDream file: options or a column construct near the start, and plain text.
pub fn looks_like(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(1024)];
    let start = head.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(head.len());
    let marked = head[start..].starts_with(b"%OP%") || head[start..].starts_with(b"%CO:");
    marked && head.iter().all(|b| *b == b'\r' || *b == b'\n' || *b == b'\t' || (0x20..0x7F).contains(b) || *b >= 0x80)
}

struct Column {
    letter: String,
    slots: Vec<Vec<Run>>,
    /// Each slot's alignment, if it said.
    centred: Vec<bool>,
}

/// Reads one slot's line: its text as runs, whether it's centred, whether it's a number, and whether a page break
/// comes before it.
fn slot(line: &str, highlights: &mut [bool; 8]) -> (Vec<Run>, bool, bool) {
    let mut b = Builder::default();
    let mut centred = false;
    let mut number = false;
    let style = |h: &[bool; 8]| Style { underline: h[1], bold: h[2], italic: h[4], subscript: h[5], superscript: h[6], ..Style::default() };
    b.style = style(highlights);
    let mut rest = line;
    while let Some(at) = rest.find('%') {
        for c in rest[..at].chars() {
            b.push(c);
        }
        let after = &rest[at + 1..];
        let Some(end) = after.find('%').filter(|end| *end <= 12 && after[..*end].chars().all(|c| c.is_ascii_alphanumeric() || c == ':' || c == ',')) else {
            b.push('%');
            rest = after;
            continue;
        };
        let construct = &after[..end];
        rest = &after[end + 1..];
        match construct {
            "PC" => b.push('%'),
            "C" => centred = true,
            "V" => number = true,
            _ if construct.starts_with('H') => {
                if let Some(n) = construct[1..].parse::<usize>().ok().filter(|n| (1..8).contains(n)) {
                    highlights[n] = !highlights[n];
                    b.style = style(highlights);
                }
            }
            // Alignment, number formats and the like change how it's shown, not what it says.
            _ => {}
        }
    }
    for c in rest.chars() {
        b.push(c);
    }
    let runs = b.finish().into_iter().find_map(|block| if let Block::Paragraph { runs, .. } = block { Some(runs) } else { None }).unwrap_or_default();
    (runs, centred, number)
}

pub fn read(bytes: &[u8]) -> Result<Doc, String> {
    let text: String = bytes.iter().map(|b| char::from(*b)).collect();
    let mut doc = Doc { format: "PipeDream".into(), ..Doc::default() };
    let mut columns: Vec<Column> = Vec::new();
    let mut header = Vec::new();
    let mut highlights = [false; 8];
    let mut numbers = 0;
    let mut page_breaks = Vec::new();
    let mut other_options = false;
    for line in text.split(['\r', '\n']) {
        if let Some(option) = line.strip_prefix("%OP%") {
            let (name, value) = option.split_at(option.len().min(2));
            match name {
                "DE" if !value.trim().is_empty() => header.push(Block::Heading { level: 1, text: value.trim().to_string() }),
                "HE" if !value.trim().is_empty() => header.push(Block::Header(value.trim().to_string())),
                "FO" if !value.trim().is_empty() => header.push(Block::Footer(value.trim().to_string())),
                _ => other_options = true,
            }
            continue;
        }
        // A column starts: its construct, and its first slot may follow on the same line.
        let mut line = line;
        if let Some(rest) = line.strip_prefix("%CO:")
            && let Some(end) = rest.find('%')
        {
            let letter = rest[..end].split(',').next().unwrap_or("").trim().to_string();
            columns.push(Column { letter, slots: Vec::new(), centred: Vec::new() });
            line = &rest[end + 1..];
            if line.is_empty() {
                continue;
            }
        }
        let Some(column) = columns.last_mut() else {
            continue;
        };
        if line.match_indices("%P").any(|(at, _)| line[at + 2..].starts_with(|c: char| c.is_ascii_digit())) {
            page_breaks.push(column.slots.len());
        }
        let (runs, centred, number) = slot(line, &mut highlights);
        numbers += usize::from(number);
        column.slots.push(runs);
        column.centred.push(centred);
    }
    // The last line ends with the file's last return, which isn't a slot.
    for column in &mut columns {
        while column.slots.last().is_some_and(Vec::is_empty) {
            column.slots.pop();
            column.centred.pop();
        }
    }
    doc.blocks = header;
    let used: Vec<&Column> = columns.iter().filter(|c| !c.slots.is_empty()).collect();
    match used.len() {
        0 => {}
        1 => {
            let column = used[0];
            for (index, runs) in column.slots.iter().enumerate() {
                if page_breaks.contains(&index) && index > 0 {
                    doc.blocks.push(Block::PageBreak);
                }
                doc.blocks.push(Block::Paragraph { runs: runs.clone(), align: if column.centred[index] { Align::Centre } else { Align::Left } });
            }
        }
        _ => {
            let rows = used.iter().map(|c| c.slots.len()).max().unwrap_or(0);
            let mut table = vec![used.iter().map(|c| c.letter.clone()).collect::<Vec<_>>()];
            for row in 0..rows {
                table.push(used.iter().map(|c| c.slots.get(row).map(|runs| runs.iter().map(|r| r.text.as_str()).collect::<String>()).unwrap_or_default()).collect());
            }
            doc.blocks.push(Block::Table(table));
        }
    }
    if numbers > 0 {
        doc.notes.push(format!("{numbers} number slot{} kept as written: PipeDream works out an expression's value when it loads the file, and doesn't save it.", if numbers == 1 { " is" } else { "s are" }));
    }
    if other_options {
        doc.notes.push("Page settings (margins, page length, line spacing) aren't carried over.".into());
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pipedream_letter_is_its_paragraphs_with_their_highlights() {
        let file = b"%OP%DEA letter\r%OP%HEMy header\r%OP%PL66\r%CO:A,72,72%Dear Sir,\r\r%H2%Thank you%H2% for 50%PC%.\r%C%Yours\r\r";
        assert!(looks_like(file));
        let doc = read(file).unwrap();
        assert_eq!(doc.blocks[0], Block::Heading { level: 1, text: "A letter".into() });
        assert_eq!(doc.blocks[1], Block::Header("My header".into()));
        let text = super::super::to_text(&doc);
        assert_eq!(text, "A letter\nDear Sir,\n\nThank you for 50%.\nYours\n");
        let markdown = super::super::to_markdown(&doc);
        assert!(markdown.contains("**Thank you**"), "{markdown}");
        assert!(markdown.contains("<p align=\"center\">Yours</p>"));
        assert!(doc.notes.iter().any(|n| n.contains("Page settings")));
    }

    #[test]
    fn a_pipedream_spreadsheet_is_a_table() {
        let file = b"%CO:A,10,9%Item\rTea\rCake\r%CO:B,8,7%Cost\r%V%1.20\r%V%2.5\r%V%B2+B3\r";
        let doc = read(file).unwrap();
        assert_eq!(doc.blocks, [Block::Table(vec![vec!["A".into(), "B".into()], vec!["Item".into(), "Cost".into()], vec!["Tea".into(), "1.20".into()], vec!["Cake".into(), "2.5".into()], vec![String::new(), "B2+B3".into()]])]);
        assert_eq!(super::super::Target::Csv.write(&doc), "Item,Cost\nTea,1.20\nCake,2.5\n,B2+B3\n");
        assert!(super::super::to_markdown(&doc).starts_with("| A | B |\n|---|---|\n| Item | Cost |"));
        assert!(doc.notes[0].starts_with("3 number slots"));
        assert!(!looks_like(b"Just some text\r"));
    }
}
