//! Documents from old word processors, converted to plain text, Markdown or RTF. Each format has a reader that turns
//! its file into a small model of paragraphs of styled text, page breaks, and headers and footers; the writers here
//! turn that model into the three outputs. What a format holds that the model can't (printer codes, fonts, margins)
//! is listed in the document's notes rather than dropped without a word.

mod wordstar;

/// How a stretch of text looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub superscript: bool,
    pub subscript: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub style: Style,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Centre,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    /// A paragraph: a line ended by a hard return. An empty one is a blank line.
    Paragraph { runs: Vec<Run>, align: Align },
    PageBreak,
    Header(String),
    Footer(String),
}

/// A converted document: what it was, its blocks, and what couldn't be carried over.
#[derive(Clone, Debug, Default)]
pub struct Doc {
    pub format: String,
    pub blocks: Vec<Block>,
    pub notes: Vec<String>,
}

/// A document format a reader here knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// WordStar 3 to 7, and HP's MemoMaker, which saves in WordStar's format.
    WordStar,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::WordStar => "WordStar",
        }
    }
}

/// Which format a file is in, if it's one of these: by its bytes, as old word processors' files seldom have
/// endings that say.
pub fn detect(_name: &str, bytes: &[u8]) -> Option<Kind> {
    wordstar::looks_like(bytes).then_some(Kind::WordStar)
}

pub fn convert(kind: Kind, bytes: &[u8]) -> Result<Doc, String> {
    match kind {
        Kind::WordStar => Ok(wordstar::read(bytes)),
    }
}

/// The three outputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Text,
    Markdown,
    Rtf,
}

impl Target {
    pub fn ending(self) -> &'static str {
        match self {
            Target::Text => "txt",
            Target::Markdown => "md",
            Target::Rtf => "rtf",
        }
    }

    pub fn write(self, doc: &Doc) -> String {
        match self {
            Target::Text => to_text(doc),
            Target::Markdown => to_markdown(doc),
            Target::Rtf => to_rtf(doc),
        }
    }
}

fn paragraph_text(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

/// Plain text: each paragraph a line, as it was typed; page breaks as form feeds, headers and footers left out.
pub fn to_text(doc: &Doc) -> String {
    let mut out = String::new();
    for block in &doc.blocks {
        match block {
            Block::Paragraph { runs, .. } => {
                out.push_str(paragraph_text(runs).trim_end());
                out.push('\n');
            }
            Block::PageBreak => out.push('\u{C}'),
            Block::Header(_) | Block::Footer(_) => {}
        }
    }
    out
}

/// Text made safe in Markdown, so nothing in it is taken for formatting.
fn markdown_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|' | '~') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Markdown: lines typed one under another (an address, a list) stay together, each ended by a hard break; a blank
/// line separates them, as it did in the document. Bold and italic are Markdown's own; underline, strikeout, and
/// superscript and subscript are the HTML that Markdown allows. Page breaks are rules.
pub fn to_markdown(doc: &Doc) -> String {
    let mut out = String::new();
    let mut group: Vec<String> = Vec::new();
    let flush = |group: &mut Vec<String>, out: &mut String| {
        if group.is_empty() {
            return;
        }
        let last = group.len() - 1;
        for (i, line) in group.drain(..).enumerate() {
            out.push_str(&line);
            out.push_str(if i == last { "\n" } else { "\\\n" });
        }
        out.push('\n');
    };
    for block in &doc.blocks {
        match block {
            Block::Paragraph { runs, align } => {
                let mut line = String::new();
                for run in runs {
                    line.push_str(&markdown_run(run));
                }
                // Leading spaces would make a code block; an indented first line is indented by nothing here.
                let line = line.trim().to_string();
                if line.is_empty() {
                    flush(&mut group, &mut out);
                } else if *align == Align::Centre {
                    flush(&mut group, &mut out);
                    out.push_str(&format!("<p align=\"center\">{line}</p>\n\n"));
                } else {
                    group.push(line);
                }
            }
            Block::PageBreak => {
                flush(&mut group, &mut out);
                out.push_str("---\n\n");
            }
            Block::Header(text) | Block::Footer(text) => {
                flush(&mut group, &mut out);
                let which = if matches!(block, Block::Header(_)) { "Header" } else { "Footer" };
                out.push_str(&format!("<!-- {which}: {} -->\n\n", text.replace("--", "- -")));
            }
        }
    }
    flush(&mut group, &mut out);
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

fn markdown_run(run: &Run) -> String {
    let text = markdown_escape(&run.text);
    // Markdown's marks don't take spaces just inside them, so those stay outside.
    let (lead, core, trail) = {
        let start = text.len() - text.trim_start().len();
        let end = text.trim_end().len();
        (&text[..start], &text[start..end.max(start)], &text[end.max(start)..])
    };
    if core.is_empty() {
        return text;
    }
    let s = run.style;
    let mut core = core.to_string();
    if s.superscript {
        core = format!("<sup>{core}</sup>");
    }
    if s.subscript {
        core = format!("<sub>{core}</sub>");
    }
    if s.underline {
        core = format!("<u>{core}</u>");
    }
    if s.strike {
        core = format!("~~{core}~~");
    }
    if s.italic {
        core = format!("*{core}*");
    }
    if s.bold {
        core = format!("**{core}**");
    }
    format!("{lead}{core}{trail}")
}

/// RTF text: its three special characters escaped, and anything past ASCII as a Unicode escape with a `?` for
/// readers that don't know them.
fn rtf_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '\t' => out.push_str("\\tab "),
            c if (c as u32) < 0x80 => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{}?", *unit as i16));
                }
            }
        }
    }
    out
}

/// RTF, as WordPad, Word and LibreOffice read it: Courier, as the documents were typed in a fixed pitch, each
/// paragraph with its alignment and its runs' styles, page breaks, and headers and footers.
pub fn to_rtf(doc: &Doc) -> String {
    let mut out = String::from("{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl{\\f0\\fmodern Courier New;}}\n\\f0\\fs20\n");
    for block in &doc.blocks {
        match block {
            Block::Header(text) => out.push_str(&format!("{{\\header\\pard {}\\par}}\n", rtf_escape(text))),
            Block::Footer(text) => out.push_str(&format!("{{\\footer\\pard {}\\par}}\n", rtf_escape(text))),
            _ => {}
        }
    }
    for block in &doc.blocks {
        match block {
            Block::Paragraph { runs, align } => {
                out.push_str(match align {
                    Align::Left => "\\pard\\ql ",
                    Align::Centre => "\\pard\\qc ",
                });
                for run in runs {
                    let s = run.style;
                    let marks: String = [(s.bold, "\\b"), (s.italic, "\\i"), (s.underline, "\\ul"), (s.strike, "\\strike"), (s.superscript, "\\super"), (s.subscript, "\\sub")].iter().filter(|(on, _)| *on).map(|(_, mark)| *mark).collect();
                    if marks.is_empty() {
                        out.push_str(&rtf_escape(&run.text));
                    } else {
                        out.push_str(&format!("{{{marks} {}}}", rtf_escape(&run.text)));
                    }
                }
                out.push_str("\\par\n");
            }
            Block::PageBreak => out.push_str("\\page\n"),
            Block::Header(_) | Block::Footer(_) => {}
        }
    }
    out.push('}');
    out
}

/// Builds paragraphs from styled characters as a reader meets them, joining runs of the same style.
#[derive(Default)]
pub struct Builder {
    pub blocks: Vec<Block>,
    runs: Vec<Run>,
    pub style: Style,
    pub align: Align,
}

impl Builder {
    pub fn push(&mut self, c: char) {
        match self.runs.last_mut() {
            Some(run) if run.style == self.style => run.text.push(c),
            _ => self.runs.push(Run { text: c.to_string(), style: self.style }),
        }
    }

    /// The text of the paragraph so far.
    pub fn current(&self) -> String {
        paragraph_text(&self.runs)
    }

    pub fn end_paragraph(&mut self) {
        let runs = std::mem::take(&mut self.runs);
        self.blocks.push(Block::Paragraph { runs, align: self.align });
        self.align = Align::Left;
    }

    pub fn block(&mut self, block: Block) {
        if !self.runs.is_empty() {
            self.end_paragraph();
        }
        self.blocks.push(block);
    }

    pub fn finish(mut self) -> Vec<Block> {
        if !self.runs.is_empty() {
            self.end_paragraph();
        }
        self.blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, style: Style) -> Run {
        Run { text: text.into(), style }
    }

    fn sample() -> Doc {
        let bold = Style { bold: true, ..Style::default() };
        let under = Style { underline: true, ..Style::default() };
        Doc {
            format: "Test".into(),
            blocks: vec![
                Block::Header("Page header".into()),
                Block::Paragraph { runs: vec![run("Mr. A. Ahab", Style::default())], align: Align::Left },
                Block::Paragraph { runs: vec![run("Titanic Cruise Lines", Style::default())], align: Align::Left },
                Block::Paragraph { runs: vec![], align: Align::Left },
                Block::Paragraph { runs: vec![run("     I am ", Style::default()), run("very", bold), run(" sorry about *the* ", Style::default()), run("deck chairs ", under), run("{costs}", Style::default())], align: Align::Left },
                Block::PageBreak,
                Block::Paragraph { runs: vec![run("Caf\u{E9}", Style::default())], align: Align::Centre },
            ],
            notes: vec![],
        }
    }

    #[test]
    fn plain_text_keeps_the_lines_as_typed() {
        assert_eq!(to_text(&sample()), "Mr. A. Ahab\nTitanic Cruise Lines\n\n     I am very sorry about *the* deck chairs {costs}\n\u{C}Caf\u{E9}\n");
    }

    #[test]
    fn markdown_keeps_lines_together_and_escapes_what_would_be_formatting() {
        assert_eq!(
            to_markdown(&sample()),
            "<!-- Header: Page header -->\n\nMr. A. Ahab\\\nTitanic Cruise Lines\n\nI am **very** sorry about \\*the\\* <u>deck chairs</u> {costs}\n\n---\n\n<p align=\"center\">Caf\u{E9}</p>\n"
        );
    }

    #[test]
    fn rtf_carries_styles_alignment_breaks_and_headers() {
        let rtf = to_rtf(&sample());
        assert!(rtf.starts_with("{\\rtf1\\ansi"));
        assert!(rtf.contains("{\\header\\pard Page header\\par}"));
        assert!(rtf.contains("\\pard\\ql      I am {\\b very} sorry about *the* {\\ul deck chairs }\\{costs\\}\\par"));
        assert!(rtf.contains("\\page\n\\pard\\qc Caf\\u233?\\par"));
        assert!(rtf.ends_with('}'));
    }
}

#[cfg(test)]
mod real_documents {
    use crate::media::{self, Entry, Volume};
    use std::path::Path;
    use std::sync::Arc;

    fn walk(volume: &dyn Volume, dir: Option<&Entry>, place: &str, out: &Path, found: &mut Vec<String>, depth: usize) {
        if depth > 8 {
            return;
        }
        for entry in volume.list(dir).unwrap_or_default() {
            let path = format!("{place}/{}", entry.name);
            if entry.is_dir {
                walk(volume, Some(&entry), &path, out, found, depth + 1);
                continue;
            }
            let Ok(bytes) = volume.read(&entry) else {
                continue;
            };
            if let Some(kind) = super::detect(&entry.open_name, &bytes) {
                let doc = super::convert(kind, &bytes).unwrap();
                let stem = path.replace(['/', '\\', ':', '!'], "_");
                for target in [super::Target::Text, super::Target::Markdown, super::Target::Rtf] {
                    std::fs::write(out.join(format!("{stem}.{}", target.ending())), target.write(&doc)).unwrap();
                }
                found.push(format!("{path}\t{}\t{}", kind.name(), doc.notes.join(" ")));
            } else if media::is_nested_image(&entry.open_name, &bytes)
                && let Ok(inner) = media::open(Arc::new(bytes), &entry.open_name)
            {
                walk(&*inner, None, &format!("{path}!"), out, found, depth + 1);
            }
        }
    }

    /// Every document a reader here knows, on every real disk in ..\testmedia, converted all three ways into
    /// ..\testmedia\limoniter-documents for comparing with other converters, and listed in its index.txt.
    #[test]
    fn real_documents_convert() {
        let root = Path::new("../testmedia");
        let Ok(folders) = std::fs::read_dir(root) else {
            return;
        };
        let out = root.join("limoniter-documents");
        std::fs::create_dir_all(&out).unwrap();
        let mut found = Vec::new();
        // Images and loose documents, one and two folders down.
        let mut files: Vec<_> = Vec::new();
        let mut pending: Vec<(std::path::PathBuf, usize)> = folders.flatten().map(|f| (f.path(), 0)).collect();
        while let Some((path, depth)) = pending.pop() {
            if path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("limoniter-")) {
                continue;
            }
            if path.is_dir() && depth < 2 {
                pending.extend(std::fs::read_dir(&path).unwrap().flatten().map(|e| (e.path(), depth + 1)));
            } else if path.is_file() && depth > 0 {
                files.push(path);
            }
        }
        files.sort();
        for file in files {
            let name = file.file_name().unwrap().to_string_lossy().into_owned();
            let bytes = std::fs::read(&file).unwrap();
            if !media::is_image(&name, &bytes[..bytes.len().min(0x8800)], bytes.len() as u64) {
                if let Some(kind) = super::detect(&name, &bytes) {
                    let doc = super::convert(kind, &bytes).unwrap();
                    for target in [super::Target::Text, super::Target::Markdown, super::Target::Rtf] {
                        std::fs::write(out.join(format!("{name}.{}", target.ending())), target.write(&doc)).unwrap();
                    }
                    found.push(format!("{name}\t{}\t{}", kind.name(), doc.notes.join(" ")));
                }
                continue;
            }
            if let Ok(volume) = media::open(Arc::new(bytes), &name) {
                walk(&*volume, None, &name, &out, &mut found, 0);
            }
        }
        std::fs::write(out.join("index.txt"), found.join("\n")).unwrap();
    }
}
