//! The formatters on the Coding tab. They only ever rearrange the space between things, and every result is
//! checked for that before it's used: if anything else would change, the text is left as it was.

use crate::code_format::{self, C_ATTACH, C_BREAK, SCRIPT};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Xml,
    XmlCompact,
    Html,
    HtmlCompact,
    Js,
    JsCompact,
    Css,
    CssCompact,
    CodeAttach,
    CodeBreak,
    Sql,
    EdiLines,
    EdiOneLine,
}

impl Kind {
    pub fn title(self) -> &'static str {
        match self {
            Kind::Xml | Kind::XmlCompact => "XML",
            Kind::Html | Kind::HtmlCompact => "HTML",
            Kind::Js | Kind::JsCompact => "JavaScript",
            Kind::Css | Kind::CssCompact => "CSS",
            Kind::CodeAttach | Kind::CodeBreak => "Code reformat",
            Kind::Sql => "SQL",
            Kind::EdiLines | Kind::EdiOneLine => "EDI",
        }
    }
}

const NOT_SAFE: &str = "That would have changed more than the layout, so nothing was changed.";

fn squeezed(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().filter(|c| !c.is_whitespace())
}

fn same_but_for_space(a: &str, b: &str) -> bool {
    squeezed(a).eq(squeezed(b))
}

/// Lays out the text. `unit` is one level of indent.
pub fn apply(kind: Kind, text: &str, unit: &str) -> Result<String, String> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let out = match kind {
        Kind::Xml => format_xml(&text, unit, false)?,
        Kind::XmlCompact => format_xml(&text, unit, true)?,
        Kind::Html => format_html(&text, unit, false),
        Kind::HtmlCompact => format_html(&text, unit, true),
        Kind::Js => code_format::format(&text, unit, &SCRIPT)?,
        Kind::JsCompact => code_format::compress(&text, &SCRIPT),
        Kind::Css => format_css(&text, unit),
        Kind::CssCompact => compress_css(&text),
        Kind::CodeAttach => code_format::format(&text, unit, &C_ATTACH)?,
        Kind::CodeBreak => code_format::format(&text, unit, &C_BREAK)?,
        Kind::Sql => format_sql(&text, unit),
        Kind::EdiLines => edi_lines(&text)?,
        Kind::EdiOneLine => edi_one_line(&text)?,
    };
    let safe = match kind {
        Kind::JsCompact => same_but_for_space(&code_format::solid_code(&text, &SCRIPT), &code_format::solid_code(&out, &SCRIPT)),
        Kind::CssCompact => same_but_for_space(&css_without_comments(&text), &out),
        Kind::Sql => squeezed(&text).flat_map(char::to_lowercase).eq(squeezed(&out).flat_map(char::to_lowercase)),
        _ => same_but_for_space(&text, &out),
    };
    if !safe {
        return Err(NOT_SAFE.to_string());
    }
    // Whether the text ends with a line break isn't a matter of layout.
    let mut out = out.trim_end_matches('\n').to_string();
    if text.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

// ---------- XML ----------

enum XmlNode {
    /// Comments, processing instructions, CDATA and DOCTYPE, exactly as written.
    Raw(String),
    Text(String),
    Element { open: String, close: Option<String>, children: Vec<XmlNode> },
}

/// The end of a tag, allowing for `>` inside quoted values.
fn tag_end(text: &str, from: usize) -> Option<usize> {
    let mut quote = None;
    for (at, c) in text[from..].char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(from + at + 1),
            _ => {}
        }
    }
    None
}

fn doctype_end(text: &str, from: usize) -> Option<usize> {
    let (mut quote, mut depth) = (None, 0);
    for (at, c) in text[from..].char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '[') => depth += 1,
            (None, ']') => depth -= 1,
            (None, '>') if depth == 0 => return Some(from + at + 1),
            _ => {}
        }
    }
    None
}

fn parse_xml(text: &str) -> Option<Vec<XmlNode>> {
    let mut stack: Vec<(String, Vec<XmlNode>)> = Vec::new();
    let mut nodes: Vec<XmlNode> = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        if !rest.starts_with('<') {
            let end = rest.find('<').map_or(text.len(), |found| at + found);
            nodes.push(XmlNode::Text(text[at..end].to_string()));
            at = end;
            continue;
        }
        let special = [("<!--", "-->"), ("<![CDATA[", "]]>"), ("<?", "?>")].iter().find(|(open, _)| rest.starts_with(open)).map(|(_, close)| *close);
        let end = if let Some(close) = special {
            at + rest.find(close)? + close.len()
        } else if rest.starts_with("<!") {
            doctype_end(text, at)?
        } else {
            tag_end(text, at)?
        };
        let raw = text[at..end].to_string();
        at = end;
        if special.is_some() || raw.starts_with("<!") {
            nodes.push(XmlNode::Raw(raw));
        } else if raw.starts_with("</") {
            let (open, parent) = stack.pop()?;
            let children = std::mem::replace(&mut nodes, parent);
            nodes.push(XmlNode::Element { open, close: Some(raw), children });
        } else if raw.ends_with("/>") {
            nodes.push(XmlNode::Element { open: raw, close: None, children: Vec::new() });
        } else {
            stack.push((raw, std::mem::take(&mut nodes)));
        }
    }
    stack.is_empty().then_some(nodes)
}

fn xml_source(nodes: &[XmlNode], out: &mut String) {
    for node in nodes {
        match node {
            XmlNode::Raw(raw) | XmlNode::Text(raw) => out.push_str(raw),
            XmlNode::Element { open, close, children } => {
                out.push_str(open);
                xml_source(children, out);
                out.push_str(close.as_deref().unwrap_or(""));
            }
        }
    }
}

/// Where text sits among the tags its spacing means something, so it's kept as written.
fn keeps_its_space(open: &str, children: &[XmlNode]) -> bool {
    open.contains("xml:space=\"preserve\"") || open.contains("xml:space='preserve'") || children.iter().any(|child| matches!(child, XmlNode::Text(text) if !text.trim().is_empty()) || matches!(child, XmlNode::Raw(raw) if raw.starts_with("<![CDATA[")))
}

fn render_xml(nodes: &[XmlNode], depth: usize, unit: &str, compact: bool, out: &mut String) {
    for node in nodes {
        let start = out.len();
        match node {
            XmlNode::Text(text) if text.trim().is_empty() => continue,
            XmlNode::Text(text) => out.push_str(text.trim()),
            XmlNode::Raw(raw) => out.push_str(raw),
            XmlNode::Element { open, close, children } => {
                out.push_str(open);
                if keeps_its_space(open, children) || children.iter().all(|child| matches!(child, XmlNode::Text(_))) {
                    if keeps_its_space(open, children) {
                        xml_source(children, out);
                    }
                } else {
                    if !compact {
                        out.push('\n');
                    }
                    render_xml(children, depth + 1, unit, compact, out);
                    if !compact {
                        out.push_str(&unit.repeat(depth));
                    }
                }
                out.push_str(close.as_deref().unwrap_or(""));
            }
        }
        if !compact {
            out.insert_str(start, &unit.repeat(depth));
            out.push('\n');
        }
    }
}

fn format_xml(text: &str, unit: &str, compact: bool) -> Result<String, String> {
    let options = || roxmltree::ParsingOptions { allow_dtd: true, ..roxmltree::ParsingOptions::default() };
    if let Err(problem) = roxmltree::Document::parse_with_options(text, options()) {
        // A selection can be part of a document, with several elements side by side.
        let body = text.trim_start().strip_prefix("<?xml").and_then(|rest| rest.split_once("?>")).map_or(text, |(_, rest)| rest);
        if roxmltree::Document::parse_with_options(&format!("<a>{body}</a>"), options()).is_err() {
            return Err(format!("That isn't well-formed XML, so it hasn't been changed.\n\n{problem}"));
        }
    }
    let nodes = parse_xml(text).ok_or("That isn't well-formed XML, so it hasn't been changed.")?;
    let mut out = String::with_capacity(text.len() * 2);
    render_xml(&nodes, 0, unit, compact, &mut out);
    Ok(out)
}

// ---------- HTML ----------

const HTML_BLOCKS: &[&str] = &[
    "html", "head", "body", "title", "meta", "link", "base", "div", "p", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "dl", "dt", "dd", "table", "caption", "colgroup", "col", "thead", "tbody", "tfoot", "tr", "td", "th", "form", "fieldset", "legend", "section", "article", "aside",
    "nav", "header", "footer", "main", "figure", "figcaption", "blockquote", "address", "hr", "select", "option", "optgroup", "details", "summary", "script", "style", "pre", "noscript",
];
const HTML_VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];
/// Kept exactly as written, inside as well.
const HTML_RAW: &[&str] = &["script", "style", "pre", "textarea"];

fn html_tag_name(raw: &str) -> String {
    raw.trim_start_matches('<').trim_start_matches('/').chars().take_while(|c| c.is_alphanumeric() || matches!(c, '-' | ':' | '_')).collect::<String>().to_lowercase()
}

/// What opening this element closes without being told, as HTML lets some be left open.
fn html_closes(top: &str, name: &str) -> bool {
    match top {
        "li" => name == "li",
        "dt" | "dd" => matches!(name, "dt" | "dd"),
        "tr" => matches!(name, "tr" | "tbody" | "tfoot"),
        "td" | "th" => matches!(name, "td" | "th" | "tr" | "tbody" | "tfoot"),
        "thead" | "tbody" | "tfoot" => matches!(name, "tbody" | "tfoot"),
        "option" => matches!(name, "option" | "optgroup"),
        "p" => HTML_BLOCKS.contains(&name),
        _ => false,
    }
}

fn collapse_space(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            in_space = true;
        } else {
            if in_space {
                out.push(' ');
            }
            in_space = false;
            out.push(c);
        }
    }
    if in_space {
        out.push(' ');
    }
    out
}

struct HtmlOut<'a> {
    lines: Vec<String>,
    cur: String,
    indent: usize,
    /// The current line is still the one a block element opened on.
    fresh: bool,
    unit: &'a str,
}

impl HtmlOut<'_> {
    fn flush(&mut self) {
        let text = self.cur.trim_end();
        if !text.is_empty() {
            self.lines.push(format!("{}{text}", self.unit.repeat(self.indent)));
        }
        self.cur.clear();
        self.fresh = false;
    }

    fn append(&mut self, text: &str, depth: usize) {
        if self.cur.is_empty() {
            self.indent = depth;
            self.cur.push_str(text.trim_start());
        } else {
            self.cur.push_str(text);
        }
    }
}

fn format_html(text: &str, unit: &str, compact: bool) -> String {
    let unit = if compact { "" } else { unit };
    let mut out = HtmlOut { lines: Vec::new(), cur: String::new(), indent: 0, fresh: false, unit };
    let mut stack: Vec<String> = Vec::new();
    let lower = text.to_ascii_lowercase();
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let tag_like = rest.starts_with('<') && rest[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!' || c == '?');
        if !tag_like {
            let from = usize::from(rest.starts_with('<'));
            let end = rest[from..].find('<').map_or(text.len(), |found| at + from + found);
            let run = collapse_space(&text[at..end]);
            if !(run == " " && out.cur.is_empty()) {
                out.append(&run, stack.len());
            }
            at = end;
            continue;
        }
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map_or(text.len(), |found| at + found + 3);
            out.flush();
            out.append(&text[at..end], stack.len());
            out.flush();
            at = end;
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            let end = tag_end(text, at).unwrap_or(text.len());
            out.flush();
            out.append(&text[at..end], stack.len());
            out.flush();
            at = end;
            continue;
        }
        let end = tag_end(text, at).unwrap_or(text.len());
        let raw = &text[at..end];
        let name = html_tag_name(raw);
        at = end;
        let block = HTML_BLOCKS.contains(&name.as_str());
        if raw.starts_with("</") {
            if !block {
                out.append(raw, stack.len());
                continue;
            }
            let found = stack.iter().rposition(|open| *open == name);
            // It only closes on the line it opened on if it's what that line opened.
            let own_line = out.fresh && found.is_some_and(|place| place + 1 == stack.len());
            if let Some(place) = found {
                stack.truncate(place);
            }
            if own_line {
                let kept = out.cur.trim_end().len();
                out.cur.truncate(kept);
                out.cur.push_str(raw);
            } else {
                out.flush();
                out.append(raw, stack.len());
            }
            out.flush();
            continue;
        }
        let void = HTML_VOID.contains(&name.as_str()) || raw.ends_with("/>");
        if HTML_RAW.contains(&name.as_str()) && !void {
            let closing = format!("</{name}");
            let close_at = lower[at..].find(&closing).map(|found| at + found);
            let finish = close_at.and_then(|found| tag_end(text, found)).unwrap_or(text.len());
            if block {
                out.flush();
            }
            out.append(&text[at - raw.len()..finish], stack.len());
            if block {
                out.flush();
            }
            at = finish;
            continue;
        }
        if !block {
            out.append(raw, stack.len());
            continue;
        }
        while stack.last().is_some_and(|top| html_closes(top, &name)) {
            stack.pop();
        }
        out.flush();
        out.append(raw, stack.len());
        if void {
            out.flush();
        } else {
            stack.push(name);
            out.fresh = true;
        }
    }
    out.flush();
    out.lines.join(if compact { "" } else { "\n" })
}

// ---------- CSS ----------

/// Goes through CSS piece by piece: a comment or string whole, or one character.
fn css_piece(text: &str) -> &str {
    if text.starts_with("/*") {
        return text.find("*/").map_or(text, |found| &text[..found + 2]);
    }
    if let Some(quote) = text.chars().next().filter(|c| matches!(c, '"' | '\'')) {
        let mut chars = text.char_indices().skip(1);
        while let Some((at, c)) = chars.next() {
            if c == '\\' {
                chars.next();
            } else if c == quote || c == '\n' {
                return &text[..at + c.len_utf8()];
            }
        }
        return text;
    }
    let size = text.chars().next().map_or(0, char::len_utf8);
    &text[..size]
}

fn css_without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let piece = css_piece(rest);
        if !piece.starts_with("/*") {
            out.push_str(piece);
        }
        rest = &rest[piece.len()..];
    }
    out
}

fn format_css(text: &str, unit: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let (mut depth, mut parens) = (0_usize, 0_usize);
    let mut rest = text;
    while !rest.is_empty() {
        let piece = css_piece(rest);
        rest = &rest[piece.len()..];
        let flush = |cur: &mut String, lines: &mut Vec<String>, depth: usize| {
            let line = cur.trim();
            if !line.is_empty() {
                lines.push(format!("{}{line}", unit.repeat(depth)));
            }
            cur.clear();
        };
        match piece {
            "{" if parens == 0 => {
                let line = format!("{}{{", if cur.trim().is_empty() { String::new() } else { format!("{} ", cur.trim()) });
                cur = line;
                flush(&mut cur, &mut lines, depth);
                depth += 1;
            }
            "}" if parens == 0 => {
                flush(&mut cur, &mut lines, depth);
                depth = depth.saturating_sub(1);
                lines.push(format!("{}}}", unit.repeat(depth)));
                if depth == 0 {
                    lines.push(String::new());
                }
            }
            ";" if parens == 0 => {
                cur.push(';');
                flush(&mut cur, &mut lines, depth);
            }
            _ if piece.starts_with("/*") => {
                let alone = cur.trim().is_empty();
                if alone {
                    flush(&mut cur, &mut lines, depth);
                    lines.push(format!("{}{piece}", unit.repeat(depth)));
                } else {
                    cur.push_str(piece);
                }
            }
            _ if piece.chars().all(char::is_whitespace) => {
                if !cur.is_empty() && !cur.ends_with(' ') {
                    cur.push(' ');
                }
            }
            _ => {
                match piece {
                    "(" => parens += 1,
                    ")" => parens = parens.saturating_sub(1),
                    _ => {}
                }
                cur.push_str(piece);
            }
        }
    }
    let last = cur.trim();
    if !last.is_empty() {
        lines.push(last.to_string());
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.join("\n")
}

/// Tidies one piece of CSS: no space after a comma, nor after the colon of a declaration, outside its strings.
fn tight_css(segment: &str, declaration: bool) -> String {
    let mut out = String::with_capacity(segment.len());
    let mut rest = segment.trim();
    let mut colon_done = !declaration;
    while !rest.is_empty() {
        let piece = css_piece(rest);
        rest = &rest[piece.len()..];
        out.push_str(piece);
        let colon = piece == ":" && !colon_done;
        if piece == "," || colon {
            rest = rest.trim_start();
            let kept = out[..out.len() - piece.len()].trim_end().len();
            out.replace_range(kept..out.len(), piece);
        }
        colon_done |= colon;
    }
    out
}

fn compress_css(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut segment = String::new();
    let mut parens = 0_usize;
    let mut rest = text;
    while !rest.is_empty() {
        let piece = css_piece(rest);
        rest = &rest[piece.len()..];
        match piece {
            "{" if parens == 0 => {
                out.push_str(&tight_css(&segment, false));
                out.push('{');
                segment.clear();
            }
            "}" | ";" if parens == 0 => {
                out.push_str(&tight_css(&segment, true));
                out.push_str(piece);
                segment.clear();
            }
            _ if piece.starts_with("/*") => {}
            _ if piece.chars().all(char::is_whitespace) => {
                if !segment.ends_with(' ') {
                    segment.push(' ');
                }
            }
            _ => {
                match piece {
                    "(" => parens += 1,
                    ")" => parens = parens.saturating_sub(1),
                    _ => {}
                }
                segment.push_str(piece);
            }
        }
    }
    out.push_str(&tight_css(&segment, false));
    out
}

// ---------- SQL ----------

/// Words made upper case. Only ones that can't be somebody's table or column name without being quoted.
const SQL_KEYWORDS: &[&str] = &[
    "select", "from", "where", "group", "by", "order", "having", "limit", "offset", "union", "except", "intersect", "all", "insert", "into", "values", "update", "set", "delete", "join", "inner", "left", "right", "outer", "full", "cross", "on", "and", "or", "not", "null", "as", "in", "is",
    "like", "between", "exists", "case", "when", "then", "else", "end", "distinct", "asc", "desc", "create", "table", "alter", "drop", "with",
];

#[derive(Clone, Debug, PartialEq)]
enum SqlTok {
    Word(String),
    Raw(String),
    Comment(String),
    Punct(char),
    Space,
}

fn lex_sql(text: &str) -> Vec<SqlTok> {
    let chars: Vec<char> = text.chars().collect();
    let piece = |from: usize, to: usize| chars[from..to.min(chars.len())].iter().collect::<String>();
    let mut toks = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        let next = chars.get(at + 1).copied();
        if c.is_whitespace() {
            while at < chars.len() && chars[at].is_whitespace() {
                at += 1;
            }
            toks.push(SqlTok::Space);
        } else if c == '-' && next == Some('-') {
            let from = at;
            while at < chars.len() && chars[at] != '\n' {
                at += 1;
            }
            toks.push(SqlTok::Comment(piece(from, at).trim_end().to_string()));
        } else if c == '/' && next == Some('*') {
            let from = at;
            at += 2;
            while at < chars.len() && !(chars[at] == '*' && chars.get(at + 1) == Some(&'/')) {
                at += 1;
            }
            at = (at + 2).min(chars.len());
            toks.push(SqlTok::Comment(piece(from, at)));
        } else if matches!(c, '\'' | '"' | '`') || (c == '[' && chars[at..].iter().take_while(|inner| **inner != '\n').any(|inner| *inner == ']')) {
            let close = if c == '[' { ']' } else { c };
            let from = at;
            at += 1;
            while at < chars.len() {
                if chars[at] == close {
                    // A doubled quote is one quote, inside the string.
                    if chars.get(at + 1) == Some(&close) {
                        at += 2;
                        continue;
                    }
                    break;
                }
                at += 1;
            }
            at = (at + 1).min(chars.len());
            toks.push(SqlTok::Raw(piece(from, at)));
        } else if c == '$' && let Some(close) = chars[at + 1..].iter().position(|inner| !(inner.is_alphanumeric() || *inner == '_')).filter(|found| chars[at + 1 + found] == '$') {
            // PostgreSQL's $$ quoting, with a name between the dollars or without.
            let tag = piece(at, at + close + 2);
            let tail = piece(at + tag.chars().count(), chars.len());
            let size = tail.find(&tag).map_or(tail.chars().count(), |found| tail[..found].chars().count() + tag.chars().count());
            let end = at + tag.chars().count() + size;
            toks.push(SqlTok::Raw(piece(at, end)));
            at = end;
        } else if c.is_alphanumeric() || matches!(c, '_' | '@' | '#' | '$') {
            let from = at;
            while at < chars.len() && (chars[at].is_alphanumeric() || matches!(chars[at], '_' | '@' | '#' | '$')) {
                at += 1;
            }
            toks.push(SqlTok::Word(piece(from, at)));
        } else {
            toks.push(SqlTok::Punct(c));
            at += 1;
        }
    }
    toks
}

fn format_sql(text: &str, unit: &str) -> String {
    let toks = lex_sql(text);
    let word_at = |at: usize| match toks.get(at) {
        Some(SqlTok::Word(word)) => Some(word.to_lowercase()),
        _ => None,
    };
    let next_word = |from: usize| toks.iter().skip(from).find(|tok| **tok != SqlTok::Space).and_then(|tok| if let SqlTok::Word(word) = tok { Some(word.to_lowercase()) } else { None });
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut indent = 0;
    // For each open bracket, whether it holds a subquery that has its own lines.
    let mut stack: Vec<bool> = Vec::new();
    let mut base = 0;
    let (mut between, mut after_join, mut after_delete, mut in_list) = (false, false, false, false);
    let flush = |cur: &mut String, lines: &mut Vec<String>, level: usize| {
        let line = cur.trim();
        if !line.is_empty() {
            lines.push(format!("{}{line}", unit.repeat(level)));
        }
        cur.clear();
    };
    for (at, tok) in toks.iter().enumerate() {
        match tok {
            SqlTok::Space => {
                if !cur.is_empty() && !cur.ends_with(' ') {
                    cur.push(' ');
                }
            }
            SqlTok::Comment(raw) => {
                let line_comment = raw.starts_with("--");
                if line_comment && !cur.trim().is_empty() {
                    cur.push_str(raw);
                    flush(&mut cur, &mut lines, indent);
                } else if line_comment {
                    flush(&mut cur, &mut lines, indent);
                    lines.push(format!("{}{raw}", unit.repeat(base)));
                } else {
                    cur.push_str(raw);
                }
            }
            SqlTok::Raw(raw) => cur.push_str(raw),
            SqlTok::Word(word) => {
                let lower = word.to_lowercase();
                let next = next_word(at + 1);
                let starts_clause = stack.last().is_none_or(|subquery| *subquery)
                    && match lower.as_str() {
                        "select" | "where" | "having" | "limit" | "offset" | "union" | "except" | "intersect" | "insert" | "values" | "update" | "set" | "delete" | "create" | "alter" | "drop" | "with" => true,
                        "from" => !after_delete,
                        "group" | "order" => next.as_deref() == Some("by"),
                        "inner" | "left" | "right" | "full" | "cross" => matches!(next.as_deref(), Some("join" | "outer")),
                        "join" => !after_join,
                        _ => false,
                    };
                if starts_clause {
                    flush(&mut cur, &mut lines, indent);
                    indent = base;
                    in_list = matches!(lower.as_str(), "select" | "group" | "order" | "set" | "values");
                }
                if stack.last().is_none_or(|subquery| *subquery) && (lower == "on" || (matches!(lower.as_str(), "and" | "or") && !between)) {
                    flush(&mut cur, &mut lines, indent);
                    indent = base + 1;
                }
                between = (between || lower == "between") && lower != "and";
                after_join = matches!(lower.as_str(), "inner" | "left" | "right" | "full" | "cross" | "outer");
                after_delete = lower == "delete";
                if SQL_KEYWORDS.contains(&lower.as_str()) {
                    cur.push_str(&word.to_uppercase());
                } else {
                    cur.push_str(word);
                }
                let _ = word_at;
            }
            SqlTok::Punct('(') => {
                let subquery = matches!(next_word(at + 1).as_deref(), Some("select" | "with"));
                cur.push('(');
                stack.push(subquery);
                if subquery {
                    flush(&mut cur, &mut lines, indent);
                    base += 1;
                    indent = base;
                }
            }
            SqlTok::Punct(')') => {
                if stack.pop() == Some(true) {
                    flush(&mut cur, &mut lines, indent);
                    base = base.saturating_sub(1);
                    indent = base;
                }
                cur.push(')');
            }
            SqlTok::Punct(',') => {
                cur.push(',');
                if in_list && stack.last().is_none_or(|subquery| *subquery) {
                    flush(&mut cur, &mut lines, indent);
                    indent = base + 1;
                }
            }
            SqlTok::Punct(';') => {
                cur.push(';');
                flush(&mut cur, &mut lines, indent);
                lines.push(String::new());
                (indent, base, in_list) = (0, 0, false);
                stack.clear();
            }
            SqlTok::Punct(other) => cur.push(*other),
        }
    }
    flush(&mut cur, &mut lines, indent);
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.join("\n")
}

// ---------- EDI ----------

const NOT_EDI: &str = "This doesn't start like an EDI file: X12 starts with ISA, and EDIFACT with UNA or UNB.";

/// How a file marks the end of a segment, and the character that makes the next one ordinary, if any.
fn edi_marks(text: &str) -> Result<(char, Option<char>), String> {
    let start = text.trim_start();
    let chars: Vec<char> = start.chars().take(120).collect();
    if start.starts_with("ISA") {
        // X12's first segment is a fixed length, and what comes straight after it is the terminator.
        return chars.get(105).copied().filter(|c| !c.is_alphanumeric()).map(|c| (c, None)).ok_or_else(|| "This X12 file's ISA segment is too short to read.".to_string());
    }
    if start.starts_with("UNA") {
        let (release, terminator) = (chars.get(6).copied(), chars.get(8).copied());
        return terminator.map(|c| (c, release.filter(|r| *r != ' '))).ok_or_else(|| "This EDIFACT file's UNA segment is too short to read.".to_string());
    }
    if start.starts_with("UNB") {
        return Ok(('\'', Some('?')));
    }
    Err(NOT_EDI.to_string())
}

fn edi_segments(text: &str) -> Result<(Vec<String>, char), String> {
    let (terminator, release) = edi_marks(text)?;
    if terminator == '\n' || terminator == '\r' {
        return Err("Here the segments end with a line break, so there's nothing to put on separate lines or join up.".to_string());
    }
    let mut segments = Vec::new();
    let mut cur = String::new();
    let start = text.trim_start();
    // UNA's own characters aren't marks: it's a fixed nine long.
    let (head, rest) = if start.starts_with("UNA") {
        let split = start.char_indices().nth(9).map_or(start.len(), |(at, _)| at);
        (Some(&start[..split]), &start[split..])
    } else {
        (None, start)
    };
    if let Some(head) = head {
        segments.push(head.to_string());
    }
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        if c == '\n' || c == '\r' {
            continue;
        }
        cur.push(c);
        if Some(c) == release {
            cur.extend(chars.next());
        } else if c == terminator {
            segments.push(std::mem::take(&mut cur));
        }
    }
    if !cur.trim().is_empty() {
        segments.push(cur);
    }
    Ok((segments, terminator))
}

fn edi_lines(text: &str) -> Result<String, String> {
    Ok(edi_segments(text)?.0.join("\n"))
}

fn edi_one_line(text: &str) -> Result<String, String> {
    Ok(edi_segments(text)?.0.concat())
}

#[cfg(test)]
#[path = "formatters_tests.rs"]
mod tests;
