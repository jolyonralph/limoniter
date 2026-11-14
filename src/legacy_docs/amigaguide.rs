//! AmigaGuide hypertext, the Amiga's help and documentation format from 1990 on. As Commodore's AmigaGuide
//! documentation (and the V39 and V40 additions) give it: a text file starting @database, made of nodes from @node
//! (a name and a title) to @endnode. Lines starting with @ are commands; in the text, @{"label" link node} is a link
//! to another node (or "file/node" in another file), and @{b}, @{i}, @{u} and their @{ub}, @{ui}, @{uu} turn bold,
//! italic and underline on and off. \@ and \\ stand for themselves. The text is ISO 8859-1.
//!
//! Each node becomes a heading of its own, and links between them links to those headings.

use super::{Align, Block, Builder, Doc, Style};

/// Whether the bytes are an AmigaGuide file: @database at the start, as AmigaGuide itself requires, or an ending of
/// .guide with nodes in it.
pub fn looks_like(name: &str, bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let start = head.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(head.len());
    if head[start..].len() >= 9 && head[start..start + 9].eq_ignore_ascii_case(b"@database") {
        return true;
    }
    let lower = name.to_lowercase();
    (lower.ends_with(".guide") || lower.ends_with(".gui")) && head.windows(5).any(|w| w.eq_ignore_ascii_case(b"@node")) && !head.contains(&0)
}

/// A command line's arguments: words, or quoted strings with spaces in them.
fn arguments(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '"' {
            chars.next();
            out.push(chars.by_ref().take_while(|c| *c != '"').collect());
        } else {
            let mut word = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                word.push(c);
                chars.next();
            }
            out.push(word);
        }
    }
    out
}

/// What was met that doesn't carry over.
#[derive(Default)]
struct Dropped {
    other_files: usize,
    missing: usize,
    commands: usize,
    attributes: usize,
}

struct Node {
    name: String,
    title: String,
    lines: Vec<String>,
    smartwrap: bool,
}

pub fn read(bytes: &[u8]) -> Doc {
    let text: String = bytes.iter().map(|c| char::from(*c)).collect();
    let mut database = String::new();
    let mut nodes: Vec<Node> = Vec::new();
    let mut loose: Vec<String> = Vec::new();
    let mut smartwrap = false;
    let mut in_node = false;
    let mut dropped = Dropped::default();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with('@') && !line.starts_with("@{") {
            let (command, rest) = line[1..].split_once(|c: char| c.is_whitespace()).unwrap_or((&line[1..], ""));
            match command.to_lowercase().as_str() {
                "database" => database = arguments(rest).into_iter().next().unwrap_or_default(),
                "node" => {
                    let args = arguments(rest);
                    let name = args.first().cloned().unwrap_or_default();
                    let title = args.get(1).cloned().unwrap_or_else(|| name.clone());
                    nodes.push(Node { name, title, lines: Vec::new(), smartwrap });
                    in_node = true;
                }
                "title" if in_node => {
                    if let (Some(node), Some(title)) = (nodes.last_mut(), arguments(rest).into_iter().next()) {
                        node.title = title;
                    }
                }
                "endnode" => in_node = false,
                "smartwrap" if in_node => {
                    if let Some(node) = nodes.last_mut() {
                        node.smartwrap = true;
                    }
                }
                "smartwrap" => smartwrap = true,
                // Remarks, and what only says how AmigaGuide shows the file or where its buttons go.
                "remark" | "rem" | "wordwrap" | "author" | "(c)" | "$ver:" | "master" | "font" | "index" | "help" | "toc" | "prev" | "next" | "keywords" | "width" | "height" | "tab" | "proportional" | "onopen" | "onclose" => {}
                _ => dropped.commands += 1,
            }
            continue;
        }
        match nodes.last_mut() {
            Some(node) if in_node => node.lines.push(line.to_string()),
            _ if !line.trim().is_empty() || !loose.is_empty() => loose.push(line.to_string()),
            _ => {}
        }
    }
    let names: Vec<String> = nodes.iter().map(|n| n.name.to_lowercase()).collect();

    let mut b = Builder::default();
    // Text outside the nodes isn't shown by AmigaGuide (writers leave remarks there), unless there are no nodes.
    if nodes.is_empty() {
        for line in &loose {
            text_line(&mut b, line, &names, &mut dropped, false);
        }
    }
    for node in &nodes {
        b.block(Block::Heading { level: 1, text: node.title.trim().to_string() });
        b.style = Style::default();
        b.align = Align::Left;
        // A node's text starts after any blank lines, and runs up to blank lines at its end.
        let first = node.lines.iter().position(|l| !l.trim().is_empty()).unwrap_or(node.lines.len());
        let last = node.lines.iter().rposition(|l| !l.trim().is_empty()).map_or(first, |n| n + 1);
        for line in &node.lines[first..last.max(first)] {
            text_line(&mut b, line, &names, &mut dropped, node.smartwrap);
        }
        if !b.current().is_empty() {
            b.end_paragraph();
        }
        // A blank line between one node's text and the next heading.
        b.end_paragraph();
    }
    let mut blocks = b.finish();
    while matches!(blocks.last(), Some(Block::Paragraph { runs, .. }) if runs.is_empty()) {
        blocks.pop();
    }
    let mut notes = Vec::new();
    if dropped.other_files > 0 {
        notes.push(format!("{} link(s) to other files or programs are kept as their text.", dropped.other_files));
    }
    if dropped.missing > 0 {
        notes.push(format!("{} link(s) to nodes not in the file are kept as their text.", dropped.missing));
    }
    if dropped.attributes > 0 {
        notes.push(format!("{} colour, font and layout change(s) aren't carried over.", dropped.attributes));
    }
    if dropped.commands > 0 {
        notes.push(format!("{} other command line(s) aren't carried over.", dropped.commands));
    }
    let format = if database.is_empty() { "AmigaGuide".to_string() } else { format!("AmigaGuide ({database})") };
    Doc { format, blocks, notes }
}

/// One line of a node's text, with its links and attributes. With smart wrapping, lines run together into
/// paragraphs that blank lines end; otherwise each line is one.
fn text_line(b: &mut Builder, line: &str, names: &[String], dropped: &mut Dropped, smartwrap: bool) {
    if smartwrap {
        if line.trim().is_empty() {
            if !b.current().is_empty() {
                b.end_paragraph();
            }
            b.end_paragraph();
            return;
        }
        if !b.current().is_empty() && !b.current().ends_with([' ', '\t']) {
            b.push(' ');
        }
    }
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '\\' if matches!(chars.get(i), Some('@' | '\\')) => {
                b.push(chars[i]);
                i += 1;
            }
            '@' if chars.get(i) == Some(&'{') => {
                let Some(close) = closing(&chars, i + 1) else {
                    b.push('@');
                    continue;
                };
                let inside: String = chars[i + 1..close].iter().collect();
                i = close + 1;
                attribute(b, &inside, names, dropped);
            }
            c => b.push(c),
        }
    }
    if !smartwrap {
        b.end_paragraph();
    }
}

/// Where an @{...} ends: its closing brace, outside quotes.
fn closing(chars: &[char], from: usize) -> Option<usize> {
    let mut quoted = false;
    for (n, c) in chars.iter().enumerate().skip(from) {
        match c {
            '"' => quoted = !quoted,
            '}' if !quoted => return Some(n),
            _ => {}
        }
    }
    None
}

fn attribute(b: &mut Builder, inside: &str, names: &[String], dropped: &mut Dropped) {
    let trimmed = inside.trim_start();
    if trimmed.starts_with('"') {
        let args = arguments(trimmed);
        let label = args.first().cloned().unwrap_or_default();
        let action = args.get(1).map(|a| a.to_lowercase()).unwrap_or_default();
        let target = args.get(2).map(|t| t.to_lowercase()).unwrap_or_default();
        let saved = b.style;
        if action == "link" || action == "alink" {
            if let Some(n) = names.iter().position(|name| *name == target) {
                b.style.link = u16::try_from(n).ok();
            } else if target.contains('/') || target.contains(':') {
                dropped.other_files += 1;
            } else {
                dropped.missing += 1;
            }
        } else {
            dropped.other_files += 1;
        }
        // Labels are often padded with spaces, to look like buttons.
        for c in label.trim().chars() {
            b.push(c);
        }
        b.style = saved;
        return;
    }
    let mut words = trimmed.split_whitespace();
    match words.next().unwrap_or("").to_lowercase().as_str() {
        "b" => b.style.bold = true,
        "ub" => b.style.bold = false,
        "i" => b.style.italic = true,
        "ui" => b.style.italic = false,
        "u" => b.style.underline = true,
        "uu" => b.style.underline = false,
        "plain" => {
            b.style.bold = false;
            b.style.italic = false;
            b.style.underline = false;
        }
        "tab" => b.push('\t'),
        "line" => b.end_paragraph(),
        "par" => {
            b.end_paragraph();
            b.end_paragraph();
        }
        "jcenter" => b.align = Align::Centre,
        "jleft" | "jright" => b.align = Align::Left,
        "amigaguide" => "AmigaGuide".chars().for_each(|c| b.push(c)),
        _ => dropped.attributes += 1,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Kind, Target, detect, to_text};

    const GUIDE: &str = "@database test.guide\n@$VER: test 1.0\n@node Main \"Test Guide\"\n\nWelcome to @{b}Limoniter@{ub}'s @{\"other node\" link second 0} and @{\"a file\" link other.guide/main}.\n  Indented \\@ sign\n@endnode\n@node second\n@title \"The Second\"\nBack to @{\" main \" link MAIN}, @{i}in italic@{ui}.\n@{fg shine}Shiny@{fg text}\n@endnode\n";

    #[test]
    fn amigaguide_nodes_become_headings_and_links_link_them() {
        assert_eq!(detect("x", GUIDE.as_bytes()), Some(Kind::AmigaGuide));
        let doc = super::read(GUIDE.as_bytes());
        assert_eq!(doc.format, "AmigaGuide (test.guide)");
        assert_eq!(
            Target::Markdown.write(&doc),
            "# Test Guide\n\nWelcome to **Limoniter**'s [other node](#the-second) and a file.\\\nIndented @ sign\n\n# The Second\n\nBack to [main](#test-guide), *in italic*.\\\nShiny\n"
        );
        assert_eq!(to_text(&doc), "Test Guide\nWelcome to Limoniter's other node and a file.\n  Indented @ sign\n\nThe Second\nBack to main, in italic.\nShiny\n");
        assert_eq!(doc.notes, ["1 link(s) to other files or programs are kept as their text.", "2 colour, font and layout change(s) aren't carried over."]);
    }

    #[test]
    fn smart_wrapped_nodes_and_broken_markup() {
        let guide = "@database x\n@smartwrap\n@node main\nOne line\nruns on.\n\nNext @{b paragraph.\n@{\"unclosed\" link\n";
        let doc = super::read(guide.as_bytes());
        assert_eq!(to_text(&doc), "main\nOne line runs on.\n\nNext @{b paragraph. @{\"unclosed\" link\n");
        assert!(!super::looks_like("readme.txt", b"Just text with @node in it"));
        assert!(super::looks_like("a.guide", b"@NODE main\n@endnode"));
    }

    /// A real guide in ..\testmedia\wp, when it's there: Zorro II's pins, from Aminet (docs/help/Zorro.lha).
    #[test]
    fn real_amigaguide() {
        let Ok(bytes) = std::fs::read("../testmedia/wp/zorro.guide") else {
            return;
        };
        let md = Target::Markdown.write(&super::read(&bytes));
        assert!(md.starts_with("# ZORRO-II\n"));
        assert!(md.contains("[the authorS](#autor)"));
        assert!(md.contains("[+5V](#5v)"));
        // A remark between nodes, which AmigaGuide doesn't show.
        assert!(!md.contains("68000 stuff"));
    }
}
