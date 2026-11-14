//! Documents from old word processors, converted to plain text, Markdown or RTF. Each format has a reader that turns
//! its file into a small model of paragraphs of styled text, page breaks, and headers and footers; the writers here
//! turn that model into the three outputs. What a format holds that the model can't (printer codes, fonts, margins)
//! is listed in the document's notes rather than dropped without a word.

mod amigaguide;
mod firstword;
mod ftxt;
mod prowrite;
mod protext;
mod finalwriter;
mod signum;
mod wordperfect;
mod wordworth;
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
    /// A link to the heading with this number, counting from the document's first.
    pub link: Option<u16>,
    /// A footnote's mark, the run's text being its label.
    pub note: bool,
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
    /// A heading, level 1 the highest.
    Heading { level: u8, text: String },
    /// A footnote or endnote, kept at the end of the document; its mark is a run styled as a note.
    Footnote { label: String, text: String },
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
    /// Commodore's IFF formatted text (FTXT), from the Amiga's clipboard, NotePad and others.
    Ftxt,
    /// IFF WORD documents: ProWrite and New Horizons' other word processors.
    ProWrite,
    AmigaGuide,
    /// Arnor's Protext, on the Amiga, Atari ST, PC and CPC.
    Protext,
    /// GST's 1st Word and 1st Word Plus, on the Atari ST and the Archimedes.
    FirstWord,
    /// WordPerfect 4 and 5, on DOS, the Amiga and the Atari ST.
    WordPerfect,
    /// Signum! 1 and 2, on the Atari ST.
    Signum,
    /// Final Writer, on the Amiga.
    FinalWriter,
    /// Wordworth, on the Amiga.
    Wordworth,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::WordStar => "WordStar",
            Kind::Ftxt => "IFF FTXT",
            Kind::ProWrite => "ProWrite",
            Kind::AmigaGuide => "AmigaGuide",
            Kind::Protext => "Protext",
            Kind::FirstWord => "1st Word",
            Kind::WordPerfect => "WordPerfect",
            Kind::Signum => "Signum!",
            Kind::FinalWriter => "Final Writer",
            Kind::Wordworth => "Wordworth",
        }
    }
}

/// Which format a file is in, if it's one of these: by its bytes, as old word processors' files seldom have
/// endings that say.
pub fn detect(name: &str, bytes: &[u8]) -> Option<Kind> {
    if let Some(kind) = detect_amiga_st(name, bytes) {
        return Some(kind);
    }
    for (looks_like, kind) in [(signum::looks_like as fn(&[u8]) -> bool, Kind::Signum), (finalwriter::looks_like, Kind::FinalWriter), (wordworth::looks_like, Kind::Wordworth), (wordperfect::looks_like, Kind::WordPerfect)] {
        if looks_like(bytes) {
            return Some(kind);
        }
    }
    wordstar::looks_like(bytes).then_some(Kind::WordStar)
}

/// The Amiga's and the Atari ST's formats: IFF forms by their type, the others by their own marks.
fn detect_amiga_st(name: &str, bytes: &[u8]) -> Option<Kind> {
    match ftxt::form_type(bytes) {
        Some(b"FTXT") => Some(Kind::Ftxt),
        Some(b"WORD") => Some(Kind::ProWrite),
        _ if amigaguide::looks_like(name, bytes) => Some(Kind::AmigaGuide),
        _ if firstword::looks_like(bytes) => Some(Kind::FirstWord),
        _ if protext::looks_like(bytes) => Some(Kind::Protext),
        _ => None,
    }
}

pub fn convert(kind: Kind, bytes: &[u8]) -> Result<Doc, String> {
    match kind {
        Kind::WordStar => Ok(wordstar::read(bytes)),
        Kind::Ftxt => ftxt::read(bytes),
        Kind::ProWrite => prowrite::read(bytes),
        Kind::AmigaGuide => Ok(amigaguide::read(bytes)),
        Kind::Protext => Ok(protext::read(bytes)),
        Kind::FirstWord => Ok(firstword::read(bytes)),
        Kind::WordPerfect => wordperfect::read(bytes),
        Kind::Signum => signum::read(bytes),
        Kind::FinalWriter => finalwriter::read(bytes),
        Kind::Wordworth => wordworth::read(bytes),
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
    runs.iter().map(|r| if r.style.note { format!("[{}]", r.text) } else { r.text.clone() }).collect()
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
            Block::Heading { text, .. } => {
                out.push_str(text);
                out.push('\n');
            }
            Block::Header(_) | Block::Footer(_) => {}
            Block::Footnote { label, text } => out.push_str(&format!("[{label}] {text}\n")),
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
    // Whether the lines gathered are a list's items, which take no hard breaks between them.
    let mut listing = false;
    let flush = |group: &mut Vec<String>, out: &mut String, listing: &mut bool| {
        let list = std::mem::take(listing);
        if group.is_empty() {
            return;
        }
        let last = group.len() - 1;
        for (i, line) in group.drain(..).enumerate() {
            out.push_str(&line);
            out.push_str(if i == last || list { "\n" } else { "\\\n" });
        }
        out.push('\n');
    };
    let anchors = heading_anchors(&doc.blocks);
    for block in &doc.blocks {
        match block {
            Block::Paragraph { runs, align } => {
                let mut line = String::new();
                let item = list_item(runs);
                let mut skip = item.as_ref().map_or(0, |(_, length)| *length);
                for run in runs {
                    // The list's own mark, which Markdown writes its own way, isn't written again.
                    let taken = skip.min(run.text.len());
                    skip -= taken;
                    if taken < run.text.len() {
                        line.push_str(&markdown_run(&Run { text: run.text[taken..].to_string(), style: run.style }, &anchors));
                    }
                }
                // Leading spaces would make a code block; an indented first line is indented by nothing here.
                let line = line.trim().to_string();
                if let Some((mark, _)) = item.filter(|_| !line.is_empty()) {
                    if !listing {
                        flush(&mut group, &mut out, &mut listing);
                        listing = true;
                    }
                    group.push(format!("{mark} {line}"));
                    continue;
                }
                if listing {
                    flush(&mut group, &mut out, &mut listing);
                }
                if line.is_empty() {
                    flush(&mut group, &mut out, &mut listing);
                } else if *align == Align::Centre {
                    flush(&mut group, &mut out, &mut listing);
                    out.push_str(&format!("<p align=\"center\">{line}</p>\n\n"));
                } else {
                    group.push(line);
                }
            }
            Block::PageBreak => {
                flush(&mut group, &mut out, &mut listing);
                out.push_str("---\n\n");
            }
            Block::Header(text) | Block::Footer(text) => {
                flush(&mut group, &mut out, &mut listing);
                let which = if matches!(block, Block::Header(_)) { "Header" } else { "Footer" };
                out.push_str(&format!("<!-- {which}: {} -->\n\n", text.replace("--", "- -")));
            }
            Block::Heading { level, text } => {
                flush(&mut group, &mut out, &mut listing);
                out.push_str(&format!("{} {}\n\n", "#".repeat(usize::from((*level).clamp(1, 6))), markdown_escape(text.trim())));
            }
            Block::Footnote { label, text } => {
                flush(&mut group, &mut out, &mut listing);
                out.push_str(&format!("[^{}]: {}\n\n", note_label(label), markdown_escape(text)));
            }
        }
    }
    flush(&mut group, &mut out, &mut listing);
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// Each heading's anchor, as Markdown viewers make them from its text: lower case, spaces as hyphens, punctuation
/// gone, and a number after any made before.
fn heading_anchors(blocks: &[Block]) -> Vec<String> {
    let mut anchors: Vec<String> = Vec::new();
    for block in blocks {
        if let Block::Heading { text, .. } = block {
            let base: String = text.trim().to_lowercase().chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')).map(|c| if c == ' ' { '-' } else { c }).collect();
            let mut anchor = base.clone();
            let mut n = 0;
            while anchors.contains(&anchor) {
                n += 1;
                anchor = format!("{base}-{n}");
            }
            anchors.push(anchor);
        }
    }
    anchors
}

/// A paragraph that's a list's item, as typed: a bullet or a number and a full stop, then a space or tab. Its mark
/// in Markdown, and how many bytes of the paragraph it took.
fn list_item(runs: &[Run]) -> Option<(String, usize)> {
    let text = paragraph_text(runs);
    let start = text.len() - text.trim_start().len();
    let rest = &text[start..];
    let mark_end = if rest.starts_with(['\u{2022}', '\u{B7}', '\u{25CF}', '-', '*']) {
        rest.chars().next()?.len_utf8()
    } else {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 || digits > 3 || !rest[digits..].starts_with(['.', ')']) {
            return None;
        }
        digits + 1
    };
    let gap = rest[mark_end..].len() - rest[mark_end..].trim_start_matches([' ', '\t']).len();
    // A tab after the mark is how word processors set out a list; a space will do for a bullet, not for a number,
    // which as often starts a sentence ("1. Introduction").
    if gap == 0 || rest[mark_end..].trim().is_empty() || (!rest[mark_end..].starts_with('\t') && rest.as_bytes()[0].is_ascii_digit()) {
        return None;
    }
    let mark = if rest.as_bytes()[0].is_ascii_digit() { format!("{}.", &rest[..mark_end - 1]) } else { "-".into() };
    Some((mark, start + mark_end + gap))
}

/// A note's label as Markdown takes it in `[^label]`.
fn note_label(label: &str) -> String {
    label.chars().filter(|c| c.is_alphanumeric()).collect()
}

fn markdown_run(run: &Run, anchors: &[String]) -> String {
    if run.style.note {
        return format!("[^{}]", note_label(&run.text));
    }
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
    if let Some(anchor) = s.link.and_then(|n| anchors.get(usize::from(n))) {
        core = format!("[{core}](#{anchor})");
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
                    // A note's mark, with the note itself where RTF keeps it.
                    if s.note {
                        let text = doc.blocks.iter().find_map(|b| match b {
                            Block::Footnote { label, text } if *label == run.text => Some(text.as_str()),
                            _ => None,
                        });
                        let mark = rtf_escape(&run.text);
                        out.push_str(&format!("{{\\super {mark}}}{{\\footnote\\pard {{\\super {mark}}} {}}}", rtf_escape(text.unwrap_or(""))));
                        continue;
                    }
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
            Block::Heading { level, text } => {
                let size = match level {
                    1 => 32,
                    2 => 28,
                    _ => 24,
                };
                out.push_str(&format!("\\pard\\ql\\keepn{{\\b\\fs{size} {}}}\\par\n", rtf_escape(text.trim())));
            }
            Block::Header(_) | Block::Footer(_) | Block::Footnote { .. } => {}
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
                let Ok(doc) = super::convert(kind, &bytes) else { continue };
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
                    let Ok(doc) = super::convert(kind, &bytes) else { continue };
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

#[cfg(test)]
mod detecting {
    use super::*;

    /// Each sample word-processor document is taken for its own format, not another's.
    #[test]
    fn each_sample_document_is_detected_as_its_own_format() {
        let folder = std::path::Path::new("../testmedia/wp");
        let expected = [
            ("1STWORD.EX", Kind::FirstWord),
            ("Printtest", Kind::FirstWord),
            ("Convert Notes", Kind::ProWrite),
            ("Memo", Kind::ProWrite),
            ("colorsdoc.prw", Kind::ProWrite),
            ("Doc.textcraft", Kind::Ftxt),
            ("TextCfraft.doc", Kind::Ftxt),
            ("PROTEXT.EX", Kind::Protext),
            ("TUTOR1", Kind::Protext),
            ("zorro.guide", Kind::AmigaGuide),
            ("amiga-wp41-report.lrn", Kind::WordPerfect),
            ("st-wp-printer.tst", Kind::WordPerfect),
            ("dos-wp42-table.lrn", Kind::WordPerfect),
            ("dos-wp50-past.wkb", Kind::WordPerfect),
            ("dos-wp51-april93.cal", Kind::WordPerfect),
            ("signum-physik.sdo", Kind::Signum),
            ("finalwriter-fax-cover", Kind::FinalWriter),
            ("wordworth2-designers", Kind::Wordworth),
            ("wordworth5-frames", Kind::Wordworth),
        ];
        for (name, kind) in expected {
            let Ok(bytes) = std::fs::read(folder.join(name)) else { continue };
            assert_eq!(detect(name, &bytes), Some(kind), "{name}");
        }
    }
}
