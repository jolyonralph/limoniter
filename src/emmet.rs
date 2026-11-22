//! Emmet: short abbreviations that grow into markup or style rules when Tab is pressed. `ul>li.item*3` becomes a
//! list of three items, and in CSS `p10-20` becomes `padding: 10px 20px;`. Written here from Emmet's documented
//! behaviour: child `>`, sibling `+`, climb `^`, multiply `*`, numbering `$`, ids and classes, attributes `[a=b]`,
//! text `{...}`, groups `(...)`, tags left out where the parent says what they are, and the usual CSS shorthands.

/// What an abbreviation is expanded for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
    /// HTML, and what has HTML in it: Vue, Svelte, PHP templates.
    Html,
    /// JSX and TSX: `class` is `className`, and empty tags close themselves.
    Jsx,
    Css,
}

impl Syntax {
    /// The syntax for a language as the highlighter names it, if Emmet works in it.
    pub fn of_language(language: &str) -> Option<Syntax> {
        match language {
            "TypeScriptReact" => Some(Syntax::Jsx),
            "CSS" | "SCSS" | "Less" | "LESS" | "Stylus" => Some(Syntax::Css),
            _ if language.starts_with("HTML") || matches!(language, "PHP" | "ASP" | "Vue Component" | "Svelte" | "Jinja2" | "Ruby Haml") => Some(Syntax::Html),
            _ => None,
        }
    }
}

/// What an abbreviation grew into, and where the cursor goes in it (a byte offset into the text).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expansion {
    pub text: String,
    pub cursor: usize,
}

// ---------- Reading an abbreviation ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Repeat {
    Count(usize),
    /// `li*` with no number, for wrapping: one for each line wrapped.
    PerLine,
}

#[derive(Clone, Debug)]
struct Element {
    name: String,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<(String, String)>,
    text: Option<String>,
    repeat: Repeat,
    children: Vec<Item>,
    /// For wrapping: the lines, one in each repeat.
    lines: Vec<String>,
}

#[derive(Clone, Debug)]
enum Item {
    Element(Element),
    Group(Vec<Item>, Repeat),
    Text(String),
}

struct Reader {
    chars: Vec<char>,
    at: usize,
}

const MOST_REPEATS: usize = 500;

impl Reader {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    /// A name, id or class: letters, digits, `-`, `_`, `:`, and `$` with its `@` options.
    fn word(&mut self) -> String {
        let mut word = String::new();
        while let Some(c) = self.peek() {
            let numbering = c == '@' && word.contains('$') && (word.ends_with('$') || word.rsplit('$').next().is_some_and(|tail| tail.starts_with('@')));
            if c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '$' | '!') || numbering || (c == '-' && word.ends_with('@')) {
                word.push(c);
                self.at += 1;
            } else {
                break;
            }
        }
        word
    }

    fn repeat(&mut self) -> Option<Repeat> {
        if self.peek() != Some('*') {
            return Some(Repeat::Count(1));
        }
        self.at += 1;
        let mut digits = String::new();
        while let Some(c) = self.peek().filter(char::is_ascii_digit) {
            digits.push(c);
            self.at += 1;
        }
        if digits.is_empty() {
            return Some(Repeat::PerLine);
        }
        let n: usize = digits.parse().ok()?;
        (1..=MOST_REPEATS).contains(&n).then_some(Repeat::Count(n))
    }

    /// Everything up to the matching close of `[` or `{`.
    fn until(&mut self, close: char) -> Option<String> {
        let mut text = String::new();
        loop {
            let c = self.peek()?;
            self.at += 1;
            if c == close {
                return Some(text);
            }
            if c == '\\' && let Some(next) = self.peek() {
                text.push(next);
                self.at += 1;
                continue;
            }
            text.push(c);
        }
    }

    fn attributes(&mut self, into: &mut Vec<(String, String)>) -> Option<()> {
        let body = self.until(']')?;
        let chars: Vec<char> = body.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            let start = i;
            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '=' {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            if name.is_empty() {
                return None;
            }
            let mut value = String::new();
            if i < chars.len() && chars[i] == '=' {
                i += 1;
                if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
                    let quote = chars[i];
                    i += 1;
                    while i < chars.len() && chars[i] != quote {
                        value.push(chars[i]);
                        i += 1;
                    }
                    i += 1;
                } else if i < chars.len() && chars[i] == '{' {
                    // A JSX expression keeps its braces and may hold spaces.
                    let mut depth = 0;
                    while i < chars.len() {
                        value.push(chars[i]);
                        match chars[i] {
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            _ => {}
                        }
                        i += 1;
                        if depth == 0 {
                            break;
                        }
                    }
                } else {
                    while i < chars.len() && !chars[i].is_whitespace() {
                        value.push(chars[i]);
                        i += 1;
                    }
                }
            }
            into.push((name, value));
        }
        Some(())
    }

    fn element(&mut self) -> Option<Item> {
        let mut e = Element { name: self.word(), id: None, classes: Vec::new(), attrs: Vec::new(), text: None, repeat: Repeat::Count(1), children: Vec::new(), lines: Vec::new() };
        let mut any = !e.name.is_empty();
        loop {
            match self.peek() {
                Some('#') => {
                    self.at += 1;
                    let id = self.word();
                    if id.is_empty() {
                        return None;
                    }
                    e.id = Some(id);
                }
                Some('.') => {
                    self.at += 1;
                    let class = self.word();
                    if class.is_empty() {
                        return None;
                    }
                    e.classes.push(class);
                }
                Some('[') => {
                    self.at += 1;
                    self.attributes(&mut e.attrs)?;
                }
                Some('{') => {
                    self.at += 1;
                    e.text = Some(self.until('}')?);
                }
                Some('*') => {
                    e.repeat = self.repeat()?;
                    break;
                }
                _ => break,
            }
            any = true;
        }
        any.then_some(Item::Element(e))
    }

    fn item(&mut self) -> Option<Item> {
        if self.peek() == Some('(') {
            self.at += 1;
            let (inner, _) = self.sequence(true)?;
            if self.peek() != Some(')') {
                return None;
            }
            self.at += 1;
            let repeat = self.repeat()?;
            return Some(Item::Group(inner, repeat));
        }
        self.element()
    }

    /// Items joined by `+` and `>`; also how many levels a `^` climbs, for whoever called.
    fn sequence(&mut self, in_group: bool) -> Option<(Vec<Item>, usize)> {
        let mut items = Vec::new();
        loop {
            items.push(self.item()?);
            match self.peek() {
                None => return Some((items, 0)),
                Some(')') if in_group => return Some((items, 0)),
                Some('+') => self.at += 1,
                Some('>') => {
                    self.at += 1;
                    let (children, climbs) = self.sequence(in_group)?;
                    match items.last_mut()? {
                        Item::Element(e) => e.children = children,
                        _ => return None,
                    }
                    if climbs > 1 {
                        return Some((items, climbs - 1));
                    }
                    if climbs == 0 || self.peek().is_none() || (self.peek() == Some(')') && in_group) {
                        return Some((items, 0));
                    }
                }
                Some('^') => {
                    let mut n = 0;
                    while self.peek() == Some('^') {
                        n += 1;
                        self.at += 1;
                    }
                    return Some((items, n));
                }
                _ => return None,
            }
        }
    }
}

fn parse(abbreviation: &str) -> Option<Vec<Item>> {
    let mut reader = Reader { chars: abbreviation.chars().collect(), at: 0 };
    let mut all = Vec::new();
    loop {
        let (mut items, climbs) = reader.sequence(false)?;
        all.append(&mut items);
        if climbs == 0 || reader.peek().is_none() {
            break;
        }
    }
    (reader.peek().is_none() && !all.is_empty()).then_some(all)
}

// ---------- Building the markup ----------

enum Out {
    Tag { name: String, attrs: Vec<(String, String)>, children: Vec<Out> },
    Text(String),
}

const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

const INLINE: &[&str] = &[
    "a", "abbr", "acronym", "applet", "b", "basefont", "bdo", "big", "br", "button", "cite", "code", "del", "dfn", "em", "font", "i", "iframe", "img", "input", "ins", "kbd", "label", "map", "object", "q", "s", "samp", "select", "small", "span", "strike", "strong", "sub", "sup", "textarea", "tt", "u", "var",
];

/// Short names that mean a tag, with the attributes it comes with.
fn snippet(name: &str) -> Option<(&'static str, &'static [(&'static str, &'static str)])> {
    Some(match name {
        "a" => ("a", &[("href", "")]),
        "a:link" => ("a", &[("href", "http://")]),
        "a:mail" => ("a", &[("href", "mailto:")]),
        "a:tel" => ("a", &[("href", "tel:+")]),
        "img" => ("img", &[("src", ""), ("alt", "")]),
        "link" => ("link", &[("rel", "stylesheet"), ("href", "")]),
        "link:css" => ("link", &[("rel", "stylesheet"), ("href", "style.css")]),
        "script:src" => ("script", &[("src", "")]),
        "form" => ("form", &[("action", "")]),
        "form:get" => ("form", &[("action", ""), ("method", "get")]),
        "form:post" => ("form", &[("action", ""), ("method", "post")]),
        "input" | "inp" => ("input", &[("type", "text")]),
        "input:c" | "input:checkbox" => ("input", &[("type", "checkbox")]),
        "input:r" | "input:radio" => ("input", &[("type", "radio")]),
        "input:p" | "input:password" => ("input", &[("type", "password")]),
        "input:s" | "input:submit" => ("input", &[("type", "submit"), ("value", "")]),
        "input:b" | "input:button" => ("input", &[("type", "button"), ("value", "")]),
        "input:h" | "input:hidden" => ("input", &[("type", "hidden")]),
        "input:email" => ("input", &[("type", "email")]),
        "input:number" => ("input", &[("type", "number")]),
        "meta:vp" => ("meta", &[("name", "viewport"), ("content", "width=device-width, initial-scale=1.0")]),
        "meta:utf" => ("meta", &[("http-equiv", "Content-Type"), ("content", "text/html;charset=UTF-8")]),
        "ifr" => ("iframe", &[("src", ""), ("frameborder", "0")]),
        "btn" => ("button", &[]),
        "btn:s" | "button:s" => ("button", &[("type", "submit")]),
        "bq" => ("blockquote", &[]),
        "fig" => ("figure", &[]),
        "figc" => ("figcaption", &[]),
        "sect" => ("section", &[]),
        "art" => ("article", &[]),
        "hdr" => ("header", &[]),
        "ftr" => ("footer", &[]),
        "adr" => ("address", &[]),
        "dlg" => ("dialog", &[]),
        "tarea" => ("textarea", &[]),
        "opt" => ("option", &[]),
        "optg" => ("optgroup", &[]),
        "str" => ("strong", &[]),
        _ => return None,
    })
}

/// Words that are tags in their own right, so Tab after one of them makes the pair.
const TAGS: &[&str] = &[
    "a", "abbr", "address", "article", "aside", "audio", "b", "blockquote", "body", "br", "button", "canvas", "caption", "cite", "code", "col", "colgroup", "dd", "details", "dialog", "div", "dl", "dt", "em", "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "head",
    "header", "hr", "html", "i", "iframe", "img", "input", "label", "legend", "li", "link", "main", "mark", "menu", "meta", "nav", "ol", "optgroup", "option", "p", "pre", "script", "section", "select", "small", "span", "strong", "style", "sub", "summary", "sup", "table", "tbody", "td", "template", "textarea", "tfoot",
    "th", "thead", "time", "title", "tr", "u", "ul", "video",
];

/// Whether a word is one of HTML's tags.
pub fn is_tag(name: &str) -> bool {
    TAGS.contains(&name)
}

/// The tag a missing name means, from the tag it goes in.
fn implicit_name(parent: &str) -> &'static str {
    match parent {
        "ul" | "ol" | "menu" => "li",
        "table" | "tbody" | "thead" | "tfoot" => "tr",
        "tr" => "td",
        "select" | "optgroup" | "datalist" => "option",
        "colgroup" => "col",
        "p" | "span" | "em" | "strong" | "b" | "i" | "a" | "label" | "small" | "button" | "cite" | "q" | "code" => "span",
        _ => "div",
    }
}

/// A number written with `$`: how many places it fills, and which way it counts.
fn number_text(text: &str, index: usize, total: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' {
            let mut width = 0;
            while i < chars.len() && chars[i] == '$' {
                width += 1;
                i += 1;
            }
            let mut reverse = false;
            let mut start = 1usize;
            if i < chars.len() && chars[i] == '@' {
                let mut j = i + 1;
                if j < chars.len() && chars[j] == '-' {
                    reverse = true;
                    j += 1;
                }
                let mut digits = String::new();
                while j < chars.len() && chars[j].is_ascii_digit() {
                    digits.push(chars[j]);
                    j += 1;
                }
                if let Ok(n) = digits.parse() {
                    start = n;
                }
                i = j;
            }
            let value = if reverse { start + total - 1 - index } else { start + index };
            out.push_str(&format!("{value:0width$}"));
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

struct Builder {
    syntax: Syntax,
}

impl Builder {
    fn build(&self, items: &[Item], parent: &str, context: (usize, usize)) -> Vec<Out> {
        let mut out = Vec::new();
        for item in items {
            match item {
                Item::Text(text) => out.push(Out::Text(text.clone())),
                Item::Group(inner, repeat) => {
                    let count = count_of(*repeat, 1);
                    for i in 0..count {
                        out.extend(self.build(inner, parent, (i, count)));
                    }
                }
                Item::Element(e) => {
                    let count = if e.lines.is_empty() { count_of(e.repeat, 1) } else { e.lines.len() };
                    for i in 0..count {
                        let here = if matches!(e.repeat, Repeat::Count(1)) && e.lines.is_empty() { context } else { (i, count) };
                        out.push(self.element(e, parent, here, i));
                    }
                }
            }
        }
        out
    }

    fn element(&self, e: &Element, parent: &str, context: (usize, usize), line: usize) -> Out {
        let numbered = |text: &str| number_text(text, context.0, context.1);
        let given = numbered(&e.name);
        let plain = given.is_empty() && e.id.is_none() && e.classes.is_empty() && e.attrs.is_empty();
        if plain && let Some(text) = &e.text && e.children.is_empty() {
            return Out::Text(numbered(text));
        }
        let (name, mut attrs): (String, Vec<(String, String)>) = if given.is_empty() {
            (implicit_name(parent).to_string(), Vec::new())
        } else if let Some((tag, defaults)) = snippet(&given) {
            (tag.to_string(), defaults.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
        } else {
            (given.clone(), Vec::new())
        };
        let set = |attrs: &mut Vec<(String, String)>, key: &str, value: String| match attrs.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => attrs.push((key.to_string(), value)),
        };
        if let Some(id) = &e.id {
            set(&mut attrs, "id", numbered(id));
        }
        if !e.classes.is_empty() {
            let classes: Vec<String> = e.classes.iter().map(|c| numbered(c)).collect();
            set(&mut attrs, "class", classes.join(" "));
        }
        for (key, value) in &e.attrs {
            set(&mut attrs, &numbered(key), numbered(value));
        }
        if self.syntax == Syntax::Jsx {
            for (key, _) in attrs.iter_mut() {
                match key.as_str() {
                    "class" => *key = "className".into(),
                    "for" => *key = "htmlFor".into(),
                    _ => {}
                }
            }
        }
        let mut children = Vec::new();
        if let Some(text) = &e.text {
            children.push(Out::Text(numbered(text)));
        }
        children.extend(self.build(&e.children, &name, context));
        if let Some(text) = e.lines.get(line) {
            children.push(Out::Text(text.clone()));
        }
        Out::Tag { name, attrs, children }
    }
}

fn count_of(repeat: Repeat, lines: usize) -> usize {
    match repeat {
        Repeat::Count(n) => n,
        Repeat::PerLine => lines,
    }
}

// ---------- Laying the markup out ----------

fn inline(out: &Out) -> bool {
    match out {
        Out::Text(_) => true,
        Out::Tag { name, .. } => INLINE.contains(&name.as_str()),
    }
}

/// Whether a piece goes on a line of its own: blocks do, and so does what follows one.
fn breaks(siblings: &[Out], i: usize) -> bool {
    match &siblings[i] {
        Out::Text(text) => text.contains('\n') || (i > 0 && !inline(&siblings[i - 1])),
        Out::Tag { .. } if !inline(&siblings[i]) => true,
        Out::Tag { .. } => {
            if i == 0 {
                siblings.iter().any(|s| !inline(s))
            } else {
                !inline(&siblings[i - 1]) || matches!(&siblings[i - 1], Out::Text(t) if t.contains('\n'))
            }
        }
    }
}

struct Writer<'a> {
    text: String,
    cursor: Option<usize>,
    indent: &'a str,
    syntax: Syntax,
}

impl Writer<'_> {
    fn newline(&mut self, level: usize) {
        self.text.push('\n');
        for _ in 0..level {
            self.text.push_str(self.indent);
        }
    }

    fn mark(&mut self) {
        if self.cursor.is_none() {
            self.cursor = Some(self.text.len());
        }
    }

    fn pieces(&mut self, pieces: &[Out], level: usize, first_on_line: bool) {
        for i in 0..pieces.len() {
            if (i > 0 || !first_on_line) && breaks(pieces, i) {
                self.newline(level);
            }
            self.piece(&pieces[i], level);
        }
    }

    fn piece(&mut self, piece: &Out, level: usize) {
        match piece {
            Out::Text(text) => {
                let mut pad = String::from("\n");
                for _ in 0..level {
                    pad.push_str(self.indent);
                }
                self.text.push_str(&text.replace('\n', &pad));
            }
            Out::Tag { name, attrs, children } => {
                self.text.push('<');
                self.text.push_str(name);
                for (key, value) in attrs {
                    self.text.push(' ');
                    self.text.push_str(key);
                    if self.syntax == Syntax::Jsx && value.starts_with('{') && value.ends_with('}') {
                        self.text.push('=');
                        self.text.push_str(value);
                    } else if value.contains('"') && !value.contains('\'') {
                        self.text.push_str(&format!("='{value}'"));
                    } else {
                        self.text.push_str("=\"");
                        if value.is_empty() {
                            self.mark();
                        }
                        self.text.push_str(&value.replace('"', "&quot;"));
                        self.text.push('"');
                    }
                }
                if VOID.contains(&name.as_str()) && children.is_empty() {
                    self.text.push_str(if self.syntax == Syntax::Jsx { " />" } else { ">" });
                    return;
                }
                self.text.push('>');
                if children.is_empty() {
                    self.mark();
                }
                let split = (0..children.len()).any(|i| breaks(children, i));
                if split {
                    self.pieces(children, level + 1, false);
                    self.newline(level);
                } else {
                    self.pieces(children, level + 1, true);
                }
                self.text.push_str(&format!("</{name}>"));
            }
        }
    }
}

const DOCUMENT: &str = "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n{i}<meta charset=\"UTF-8\">\n{i}<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n{i}<title>Document</title>\n</head>\n<body>\n{i}\n</body>\n</html>";

fn markup(abbreviation: &str, syntax: Syntax, indent: &str, wrapped: Option<&str>) -> Option<Expansion> {
    if wrapped.is_none() && syntax == Syntax::Html && matches!(abbreviation, "!" | "html:5" | "doc") {
        let text = DOCUMENT.replace("{i}", indent);
        let cursor = text.find("<body>\n").map_or(0, |at| at + 7) + indent.len();
        return Some(Expansion { text, cursor });
    }
    let mut items = parse(abbreviation)?;
    if let Some(content) = wrapped {
        let lines: Vec<String> = content.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
        if !put_inside(&mut items, content.trim_end(), &lines) {
            return None;
        }
    }
    // Names that can't be a tag are left alone, so an ordinary word with a dot in it isn't turned into markup.
    if !valid_names(&items) {
        return None;
    }
    let built = Builder { syntax }.build(&items, "", (0, 1));
    let mut writer = Writer { text: String::new(), cursor: None, indent, syntax };
    writer.pieces(&built, 0, true);
    let cursor = writer.cursor.unwrap_or(writer.text.len());
    Some(Expansion { text: writer.text, cursor })
}

fn valid_names(items: &[Item]) -> bool {
    items.iter().all(|item| match item {
        Item::Text(_) => true,
        Item::Group(inner, _) => valid_names(inner),
        Item::Element(e) => (e.name.is_empty() || e.name.chars().next().is_some_and(char::is_alphabetic)) && valid_names(&e.children),
    })
}

/// Puts wrapped text inside the last element of an abbreviation. A `*` with no number there makes one for each line.
fn put_inside(items: &mut [Item], content: &str, lines: &[String]) -> bool {
    match items.last_mut() {
        Some(Item::Element(e)) => {
            if !e.children.is_empty() {
                return put_inside(&mut e.children, content, lines);
            }
            if e.repeat == Repeat::PerLine && !lines.is_empty() {
                e.lines = lines.to_vec();
            } else {
                e.children.push(Item::Text(content.to_string()));
            }
            true
        }
        Some(Item::Group(inner, _)) => put_inside(inner, content, lines),
        _ => false,
    }
}

// ---------- CSS ----------

const PROPERTIES: &[(&str, &str)] = &[
    ("m", "margin"), ("mt", "margin-top"), ("mr", "margin-right"), ("mb", "margin-bottom"), ("ml", "margin-left"),
    ("p", "padding"), ("pt", "padding-top"), ("pr", "padding-right"), ("pb", "padding-bottom"), ("pl", "padding-left"),
    ("w", "width"), ("h", "height"), ("maw", "max-width"), ("mah", "max-height"), ("miw", "min-width"), ("mih", "min-height"),
    ("t", "top"), ("r", "right"), ("b", "bottom"), ("l", "left"), ("z", "z-index"),
    ("fz", "font-size"), ("fw", "font-weight"), ("ff", "font-family"), ("fs", "font-style"), ("lh", "line-height"), ("ls", "letter-spacing"),
    ("c", "color"), ("bg", "background"), ("bgc", "background-color"), ("bgi", "background-image"), ("bgs", "background-size"),
    ("bd", "border"), ("bdt", "border-top"), ("bdr", "border-right"), ("bdb", "border-bottom"), ("bdl", "border-left"), ("bdrs", "border-radius"), ("bdc", "border-color"),
    ("op", "opacity"), ("ov", "overflow"), ("ta", "text-align"), ("td", "text-decoration"), ("tt", "text-transform"),
    ("d", "display"), ("pos", "position"), ("fl", "float"), ("cur", "cursor"), ("v", "visibility"), ("va", "vertical-align"),
    ("jc", "justify-content"), ("ai", "align-items"), ("as", "align-self"), ("fxd", "flex-direction"), ("fxw", "flex-wrap"), ("fx", "flex"), ("gap", "gap"), ("order", "order"),
    ("bxsh", "box-shadow"), ("trs", "transition"), ("trf", "transform"), ("o", "outline"), ("whs", "white-space"),
];

/// Shorthands that are a whole declaration.
const DECLARATIONS: &[(&str, &str, &str)] = &[
    ("df", "display", "flex"), ("dif", "display", "inline-flex"), ("dg", "display", "grid"), ("dn", "display", "none"), ("db", "display", "block"), ("dib", "display", "inline-block"), ("di", "display", "inline"),
    ("posa", "position", "absolute"), ("posr", "position", "relative"), ("posf", "position", "fixed"), ("poss", "position", "sticky"),
    ("jcc", "justify-content", "center"), ("jcsb", "justify-content", "space-between"), ("jcsa", "justify-content", "space-around"), ("jcfe", "justify-content", "flex-end"), ("jcfs", "justify-content", "flex-start"),
    ("aic", "align-items", "center"), ("aifs", "align-items", "flex-start"), ("aife", "align-items", "flex-end"),
    ("tac", "text-align", "center"), ("tal", "text-align", "left"), ("tar", "text-align", "right"),
    ("fwb", "font-weight", "bold"), ("fsi", "font-style", "italic"), ("tdn", "text-decoration", "none"), ("tdu", "text-decoration", "underline"),
    ("ovh", "overflow", "hidden"), ("ova", "overflow", "auto"), ("fxdc", "flex-direction", "column"), ("fxdr", "flex-direction", "row"),
    ("bxzb", "box-sizing", "border-box"), ("curp", "cursor", "pointer"),
];

/// Words after a colon, short for a property's values.
fn keyword(property: &str, short: &str) -> Option<&'static str> {
    let table: &[(&str, &str)] = match property {
        "display" => &[("n", "none"), ("b", "block"), ("f", "flex"), ("i", "inline"), ("ib", "inline-block"), ("g", "grid"), ("if", "inline-flex")],
        "position" => &[("a", "absolute"), ("r", "relative"), ("f", "fixed"), ("s", "sticky"), ("st", "static")],
        "text-align" => &[("c", "center"), ("l", "left"), ("r", "right"), ("j", "justify")],
        "overflow" => &[("h", "hidden"), ("a", "auto"), ("s", "scroll"), ("v", "visible")],
        "font-weight" => &[("b", "bold"), ("n", "normal")],
        "font-style" => &[("i", "italic"), ("n", "normal")],
        "cursor" => &[("p", "pointer"), ("d", "default")],
        "float" => &[("l", "left"), ("r", "right"), ("n", "none")],
        "justify-content" | "align-items" | "align-self" => &[("c", "center"), ("s", "flex-start"), ("e", "flex-end"), ("sb", "space-between"), ("sa", "space-around"), ("st", "stretch")],
        "flex-direction" => &[("r", "row"), ("c", "column")],
        "visibility" => &[("v", "visible"), ("h", "hidden")],
        _ => &[],
    };
    table.iter().find(|(k, _)| *k == short).map(|(_, v)| *v)
}

/// Properties whose bare numbers have no unit.
fn unitless(property: &str) -> bool {
    matches!(property, "z-index" | "opacity" | "font-weight" | "line-height" | "flex" | "order" | "zoom")
}

fn css_value(property: &str, text: &str) -> Option<String> {
    if let Some(hex) = text.strip_prefix('#') {
        if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        return Some(match hex.len() {
            1 => format!("#{}", hex.repeat(6)),
            2 => format!("#{}", hex.repeat(3)),
            _ => format!("#{hex}"),
        });
    }
    // Pieces are parted by `-`; a `-` at the start of one is its minus sign.
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if c == '-' && !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    tokens.push(current);
    let mut parts = Vec::new();
    for token in tokens {
        if token.is_empty() {
            return None;
        }
        let digits = token.trim_start_matches('-');
        if digits.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
            let split = digits.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(digits.len());
            let (number, unit) = digits.split_at(split);
            let number = if number.starts_with('.') { format!("0{number}") } else { number.to_string() };
            if number.matches('.').count() > 1 {
                return None;
            }
            let sign = if token.starts_with('-') { "-" } else { "" };
            let unit = match unit {
                "" if unitless(property) || number.trim_start_matches('0').trim_start_matches('.').is_empty() => "",
                "" => "px",
                "p" => "%",
                "e" => "em",
                "x" => "ex",
                "r" => "rem",
                other => other,
            };
            parts.push(format!("{sign}{number}{unit}"));
        } else if token.chars().all(|c| c.is_ascii_alphabetic()) {
            parts.push(keyword(property, &token).map_or(token.clone(), str::to_string));
        } else {
            return None;
        }
    }
    Some(parts.join(" "))
}

fn css_one(abbreviation: &str) -> Option<(String, Option<usize>)> {
    let (body, important) = match abbreviation.strip_suffix('!') {
        Some(rest) => (rest, " !important"),
        None => (abbreviation, ""),
    };
    if let Some((_, property, value)) = DECLARATIONS.iter().find(|(short, _, _)| *short == body) {
        return Some((format!("{property}: {value}{important};"), None));
    }
    let at = body.char_indices().find(|&(i, c)| {
        let next = body[i + c.len_utf8()..].chars().next();
        c.is_ascii_digit() || matches!(c, '#' | ':') || (c == '.' && next.is_some_and(|n| n.is_ascii_digit())) || (c == '-' && next.is_some_and(|n| n.is_ascii_digit() || n == '-'))
    });
    let (name, value) = match at {
        Some((i, _)) => (&body[..i], body[i..].trim_start_matches(':')),
        None => (body, ""),
    };
    let property = PROPERTIES.iter().find(|(short, _)| *short == name).map(|(_, p)| *p)?;
    if value.is_empty() {
        let line = format!("{property}: ;");
        let cursor = property.len() + 2;
        return Some((line, Some(cursor)));
    }
    let value = css_value(property, value)?;
    Some((format!("{property}: {value}{important};"), None))
}

fn css(abbreviation: &str, indent: &str) -> Option<Expansion> {
    let _ = indent;
    let mut lines = Vec::new();
    let mut cursor = None;
    let mut so_far = 0;
    for part in abbreviation.split('+') {
        let (line, at) = css_one(part)?;
        if cursor.is_none() {
            cursor = at.map(|c| so_far + c);
        }
        so_far += line.len() + 1;
        lines.push(line);
    }
    let text = lines.join("\n");
    let cursor = cursor.unwrap_or(text.len());
    Some(Expansion { text, cursor })
}

// ---------- Finding an abbreviation in the text ----------

/// Expands an abbreviation, laying lines after the first out with `indent` for each level.
pub fn expand(abbreviation: &str, syntax: Syntax, indent: &str) -> Option<Expansion> {
    match syntax {
        Syntax::Css => css(abbreviation, indent),
        _ => markup(abbreviation, syntax, indent, None),
    }
}

/// An abbreviation wrapped round some text: the text goes inside the innermost element, and `li*` makes an `li`
/// for every line. Only for markup.
pub fn wrap(abbreviation: &str, content: &str, syntax: Syntax, indent: &str) -> Option<Expansion> {
    if syntax == Syntax::Css || content.trim().is_empty() {
        return None;
    }
    markup(abbreviation, syntax, indent, Some(content))
}

/// Lays a multi-line expansion out where it's going: every line after the first starts with the line's own indent.
pub fn indented(expansion: Expansion, base: &str) -> Expansion {
    if base.is_empty() || !expansion.text.contains('\n') {
        return expansion;
    }
    let before = &expansion.text[..expansion.cursor];
    let lines_before = before.matches('\n').count();
    Expansion { text: expansion.text.replace('\n', &format!("\n{base}")), cursor: expansion.cursor + lines_before * base.len() }
}

/// Where the abbreviation ending a stretch of text starts (a byte offset), if there is one. In markup it stops at
/// the end of the last tag, so `<div>ul>li` means `ul>li`; inside an unfinished tag there's none.
pub fn abbreviation_start(before: &str, syntax: Syntax) -> Option<usize> {
    if syntax == Syntax::Css {
        let start = before.rfind(|c: char| c.is_whitespace() || matches!(c, ';' | '{' | '}')).map_or(0, |i| i + 1);
        return (start < before.len()).then_some(start);
    }
    let mut bound = 0;
    let mut in_tag = false;
    for (i, c) in before.char_indices() {
        if c == '<' && !in_tag {
            in_tag = true;
        } else if c == '>' && in_tag {
            in_tag = false;
            bound = i + 1;
        }
    }
    if in_tag {
        return None;
    }
    let chars: Vec<(usize, char)> = before[bound..].char_indices().map(|(i, c)| (i + bound, c)).collect();
    let mut i = chars.len();
    let mut depth = 0;
    let mut start = chars.len();
    while i > 0 {
        let (_, c) = chars[i - 1];
        match c {
            '}' | ']' => {
                let open = if c == '}' { '{' } else { '[' };
                let Some(j) = chars[..i - 1].iter().rposition(|&(_, x)| x == open) else { break };
                i = j;
                start = j;
                continue;
            }
            ')' => depth += 1,
            '(' if depth == 0 => break,
            '(' => depth -= 1,
            c if c.is_alphanumeric() || "-_:$@#.>+^*!".contains(c) => {}
            _ => break,
        }
        i -= 1;
        start = i;
    }
    if depth != 0 {
        return None;
    }
    // Something left over from before, such as a lone `>`, isn't the start.
    while start < chars.len() && !(chars[start].1.is_alphanumeric() || matches!(chars[start].1, '.' | '#' | '[' | '{' | '(' | '!' | '$')) {
        start += 1;
    }
    (start < chars.len()).then(|| chars[start].0)
}

/// Whether a lone word is worth expanding: a tag, a short name for one, or something with Emmet's marks in it.
fn worth_expanding(abbreviation: &str) -> bool {
    abbreviation.contains(['>', '+', '^', '*', '.', '#', '[', '{', '(', '$']) || abbreviation == "!" || TAGS.contains(&abbreviation) || snippet(abbreviation).is_some() || abbreviation == "html:5"
}

/// What Tab does after `before` (the text of the line up to the cursor): the number of characters to replace and
/// what to put instead, already laid out under the line's own indent. None where there's no abbreviation, so Tab
/// can do what it usually does.
pub fn expand_before(before: &str, syntax: Syntax, indent: &str) -> Option<(usize, Expansion)> {
    let start = abbreviation_start(before, syntax)?;
    let abbreviation = &before[start..];
    if syntax != Syntax::Css && !worth_expanding(abbreviation) {
        return None;
    }
    if syntax == Syntax::Jsx {
        let head = before[..start].trim_end();
        if !(head.is_empty() || ["(", ">", "return", "=>", "&&", "||", "?", ":", "{"].iter().any(|e| head.ends_with(e))) {
            return None;
        }
    }
    let base: String = before.chars().take_while(|c| c.is_whitespace()).collect();
    let expansion = expand(abbreviation, syntax, indent)?;
    Some((abbreviation.chars().count(), indented(expansion, &base)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(abbreviation: &str) -> String {
        expand(abbreviation, Syntax::Html, "\t").unwrap_or_else(|| panic!("{abbreviation} didn't expand")).text
    }

    #[test]
    fn tags_ids_and_classes() {
        assert_eq!(html("div"), "<div></div>");
        assert_eq!(html("#header"), "<div id=\"header\"></div>");
        assert_eq!(html(".title"), "<div class=\"title\"></div>");
        assert_eq!(html("p.name.second"), "<p class=\"name second\"></p>");
        assert_eq!(html("a#x.y"), "<a href=\"\" id=\"x\" class=\"y\"></a>");
    }

    #[test]
    fn children_siblings_and_climbing() {
        assert_eq!(html("div+div>p>span+em"), "<div></div>\n<div>\n\t<p><span></span><em></em></p>\n</div>");
        assert_eq!(html("div+div>p>span+em^bq"), "<div></div>\n<div>\n\t<p><span></span><em></em></p>\n\t<blockquote></blockquote>\n</div>");
        assert_eq!(html("div+div>p>span+em^^bq"), "<div></div>\n<div>\n\t<p><span></span><em></em></p>\n</div>\n<blockquote></blockquote>");
    }

    #[test]
    fn multiplying_and_numbering() {
        assert_eq!(html("ul>li.item*3"), "<ul>\n\t<li class=\"item\"></li>\n\t<li class=\"item\"></li>\n\t<li class=\"item\"></li>\n</ul>");
        assert_eq!(html("ul>li.item$*2"), "<ul>\n\t<li class=\"item1\"></li>\n\t<li class=\"item2\"></li>\n</ul>");
        assert_eq!(html("h$[title=item$]{Header $}*3"), "<h1 title=\"item1\">Header 1</h1>\n<h2 title=\"item2\">Header 2</h2>\n<h3 title=\"item3\">Header 3</h3>");
        assert_eq!(html("b.n$$$*2"), "<b class=\"n001\"></b><b class=\"n002\"></b>");
        assert_eq!(html("b.n$@-*3"), "<b class=\"n3\"></b><b class=\"n2\"></b><b class=\"n1\"></b>");
        assert_eq!(html("b.n$@3*3"), "<b class=\"n3\"></b><b class=\"n4\"></b><b class=\"n5\"></b>");
        assert_eq!(html("b.n$@-3*3"), "<b class=\"n5\"></b><b class=\"n4\"></b><b class=\"n3\"></b>");
        assert_eq!(html("ul>li*2>b{$}"), "<ul>\n\t<li><b>1</b></li>\n\t<li><b>2</b></li>\n</ul>", "children are numbered by the parent's count");
    }

    #[test]
    fn groups() {
        let expected = "<div>\n\t<dl>\n\t\t<dt></dt>\n\t\t<dd></dd>\n\t\t<dt></dt>\n\t\t<dd></dd>\n\t</dl>\n</div>\n<footer>\n\t<p></p>\n</footer>";
        assert_eq!(html("(div>dl>(dt+dd)*2)+footer>p"), expected);
    }

    #[test]
    fn attributes_and_text() {
        assert_eq!(html("a[title=\"Hello world\" rel]"), "<a href=\"\" title=\"Hello world\" rel=\"\"></a>");
        assert_eq!(html("a[href=#]{click}"), "<a href=\"#\">click</a>");
        assert_eq!(html("p>{Click }+a{here}+{ to continue}"), "<p>Click <a href=\"\">here</a> to continue</p>");
        assert_eq!(html("input[type=checkbox disabled]"), "<input type=\"checkbox\" disabled=\"\">");
    }

    #[test]
    fn tags_are_guessed_from_where_they_are() {
        assert_eq!(html("ul>.item"), "<ul>\n\t<li class=\"item\"></li>\n</ul>");
        assert_eq!(html("table>.row>.col"), "<table>\n\t<tr class=\"row\">\n\t\t<td class=\"col\"></td>\n\t</tr>\n</table>");
        assert_eq!(html("select>.o"), "<select>\n\t<option class=\"o\"></option>\n</select>");
        assert_eq!(html("p>.x"), "<p><span class=\"x\"></span></p>");
        assert_eq!(html("form>.name>input"), "<form action=\"\">\n\t<div class=\"name\"><input type=\"text\"></div>\n</form>");
    }

    #[test]
    fn short_names_and_the_page() {
        assert_eq!(html("img"), "<img src=\"\" alt=\"\">");
        assert_eq!(html("bq"), "<blockquote></blockquote>");
        assert_eq!(html("link:css"), "<link rel=\"stylesheet\" href=\"style.css\">");
        let page = html("!");
        assert!(page.starts_with("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n\t<meta charset=\"UTF-8\">"), "{page}");
        assert!(page.ends_with("</body>\n</html>"));
    }

    #[test]
    fn the_cursor_goes_to_the_first_empty_place() {
        let e = expand("ul>li*2", Syntax::Html, "\t").unwrap();
        assert_eq!(&e.text[..e.cursor], "<ul>\n\t<li>");
        let e = expand("a", Syntax::Html, "\t").unwrap();
        assert_eq!(&e.text[..e.cursor], "<a href=\"");
        let e = expand("p{hi}", Syntax::Html, "\t").unwrap();
        assert_eq!(e.cursor, e.text.len());
    }

    #[test]
    fn jsx_changes_class_for_and_empty_tags() {
        let jsx = |a: &str| expand(a, Syntax::Jsx, "  ").unwrap().text;
        assert_eq!(jsx("div.card>label[for=x]+br"), "<div className=\"card\"><label htmlFor=\"x\"></label><br /></div>");
        assert_eq!(jsx("img"), "<img src=\"\" alt=\"\" />");
        assert_eq!(jsx("Button[onClick={go}]"), "<Button onClick={go}></Button>");
    }

    #[test]
    fn wrapping_puts_the_text_inside() {
        assert_eq!(wrap("div", "hello", Syntax::Html, "\t").unwrap().text, "<div>hello</div>");
        assert_eq!(wrap("ul>li*", "one\ntwo", Syntax::Html, "\t").unwrap().text, "<ul>\n\t<li>one</li>\n\t<li>two</li>\n</ul>");
        assert_eq!(wrap("div>p", "one\ntwo", Syntax::Html, "\t").unwrap().text, "<div>\n\t<p>\n\t\tone\n\t\ttwo\n\t</p>\n</div>");
        assert_eq!(wrap("a", "   ", Syntax::Html, "\t"), None);
    }

    #[test]
    fn bad_abbreviations_are_refused() {
        for bad in ["", "div>", ">div", "a..b", "p{open", "p[x", "(a+b", "a)", "p*0", "p*9999", "1a"] {
            assert!(expand(bad, Syntax::Html, "\t").is_none(), "{bad:?}");
        }
    }

    fn style(abbreviation: &str) -> String {
        expand(abbreviation, Syntax::Css, "\t").unwrap_or_else(|| panic!("{abbreviation} didn't expand")).text
    }

    #[test]
    fn css_shorthands() {
        assert_eq!(style("m10"), "margin: 10px;");
        assert_eq!(style("p10-20"), "padding: 10px 20px;");
        assert_eq!(style("m-10"), "margin: -10px;");
        assert_eq!(style("m0-auto"), "margin: 0 auto;");
        assert_eq!(style("m10--5"), "margin: 10px -5px;");
        assert_eq!(style("df"), "display: flex;");
        assert_eq!(style("w100p"), "width: 100%;");
        assert_eq!(style("h2r"), "height: 2rem;");
        assert_eq!(style("fz1.5e"), "font-size: 1.5em;");
        assert_eq!(style("bgc#fff"), "background-color: #fff;");
        assert_eq!(style("c#1"), "color: #111111;");
        assert_eq!(style("c#e0"), "color: #e0e0e0;");
        assert_eq!(style("z10"), "z-index: 10;");
        assert_eq!(style("op.5"), "opacity: 0.5;");
        assert_eq!(style("lh1.5"), "line-height: 1.5;");
        assert_eq!(style("w100px"), "width: 100px;");
        assert_eq!(style("d:b"), "display: block;");
        assert_eq!(style("m10!"), "margin: 10px !important;");
        assert_eq!(style("posa+t0+l0"), "position: absolute;\ntop: 0;\nleft: 0;");
        let e = expand("m", Syntax::Css, "\t").unwrap();
        assert_eq!(e.text, "margin: ;");
        assert_eq!(e.cursor, 8);
        assert!(expand("banana", Syntax::Css, "\t").is_none());
    }

    #[test]
    fn the_abbreviation_is_found_before_the_cursor() {
        assert_eq!(abbreviation_start("  ul>li*3", Syntax::Html), Some(2));
        assert_eq!(abbreviation_start("<div>ul>li", Syntax::Html), Some(5), "after the tag before it");
        assert_eq!(abbreviation_start("<div cla", Syntax::Html), None, "inside a tag");
        assert_eq!(abbreviation_start("text p{a b}+i", Syntax::Html), Some(5), "text holds spaces");
        assert_eq!(abbreviation_start("x (a+b)*2", Syntax::Html), Some(2));
        assert_eq!(abbreviation_start("  m10", Syntax::Css), Some(2));
        assert_eq!(abbreviation_start("a { m10", Syntax::Css), Some(4));
    }

    #[test]
    fn tab_expands_only_what_is_meant() {
        let (n, e) = expand_before("\t\tul>li", Syntax::Html, "\t").unwrap();
        assert_eq!(n, 5);
        assert_eq!(e.text, "<ul>\n\t\t\t<li></li>\n\t\t</ul>", "later lines keep the line's indent");
        assert!(expand_before("hello", Syntax::Html, "\t").is_none(), "an ordinary word");
        assert!(expand_before("end.", Syntax::Html, "\t").is_none());
        assert!(expand_before("let x = foo.bar", Syntax::Jsx, "  ").is_none(), "code that isn't markup");
        assert!(expand_before("  return ul>li", Syntax::Jsx, "  ").is_some());
        assert!(expand_before("  div.a", Syntax::Jsx, "  ").is_some());
        assert_eq!(expand_before("a{ m10", Syntax::Css, "\t").unwrap().1.text, "margin: 10px;");
        // The cursor stays right after the lines are indented.
        let (_, e) = expand_before("  ul>li", Syntax::Html, "\t").unwrap();
        assert_eq!(&e.text[..e.cursor], "<ul>\n  \t<li>");
    }
}
