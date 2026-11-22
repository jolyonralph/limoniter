//! HTML and XML tags: the closing tag put in as an opening one is typed, the partner renamed as one's name is
//! changed, and the pair found for highlighting. Tags are read a line at a time; a tag split over lines is passed
//! over, as are comments' contents.

/// HTML's elements that have no closing tag.
const VOID: [&str; 14] = ["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

/// The most lines looked through for a partner.
const MOST_LINES: usize = 5000;

/// A tag on a line: where its name is (by character), the name, and whether it's a closing tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub name: String,
    pub closing: bool,
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.')
}

/// Every whole tag on a line, opening and closing; self-closing ones (<br/>) and comments, declarations and
/// processing instructions are left out.
pub fn tags_in(line: &str, number: usize) -> Vec<Tag> {
    let chars: Vec<char> = line.chars().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '<' {
            i += 1;
            continue;
        }
        // A comment's contents aren't tags.
        if chars[i..].starts_with(&['<', '!', '-', '-']) {
            let rest: String = chars[i + 4..].iter().collect();
            match rest.find("-->") {
                Some(end) => {
                    i += 4 + rest[..end].chars().count() + 3;
                    continue;
                }
                None => break,
            }
        }
        let closing = chars.get(i + 1) == Some(&'/');
        let start = i + 1 + usize::from(closing);
        let mut end = start;
        while end < chars.len() && is_name_char(chars[end]) {
            end += 1;
        }
        if end == start || !chars[start].is_alphabetic() {
            i += 1;
            continue;
        }
        // To the tag's end, past quoted values.
        let mut j = end;
        let mut quote = None;
        while j < chars.len() {
            match (quote, chars[j]) {
                (None, '"' | '\'') => quote = Some(chars[j]),
                (Some(q), c) if c == q => quote = None,
                (None, '>') => break,
                _ => {}
            }
            j += 1;
        }
        if j >= chars.len() {
            break;
        }
        let self_closing = chars[j - 1] == '/';
        if !self_closing {
            found.push(Tag { line: number, start, end, name: chars[start..end].iter().collect(), closing });
        }
        i = j + 1;
    }
    found
}

/// The tag whose name the cursor is in or just after, if any.
pub fn tag_at(line: &str, number: usize, column: usize) -> Option<Tag> {
    tags_in(line, number).into_iter().find(|t| column >= t.start && column <= t.end)
}

/// A tag's partner: for an opening tag, its closing one further on, and for a closing tag, its opening one before,
/// counting tags of the same name nested between.
pub fn partner(tag: &Tag, line_count: usize, line: &dyn Fn(usize) -> String) -> Option<Tag> {
    let mut depth = 0usize;
    if !tag.closing {
        for number in tag.line..line_count.min(tag.line + MOST_LINES) {
            for other in tags_in(&line(number), number) {
                if number == tag.line && other.start <= tag.start {
                    continue;
                }
                if other.name != tag.name {
                    continue;
                }
                if !other.closing {
                    depth += 1;
                } else if depth == 0 {
                    return Some(other);
                } else {
                    depth -= 1;
                }
            }
        }
    } else {
        for number in (tag.line.saturating_sub(MOST_LINES)..=tag.line).rev() {
            for other in tags_in(&line(number), number).into_iter().rev() {
                if number == tag.line && other.start >= tag.start {
                    continue;
                }
                if other.name != tag.name {
                    continue;
                }
                if other.closing {
                    depth += 1;
                } else if depth == 0 {
                    return Some(other);
                } else {
                    depth -= 1;
                }
            }
        }
    }
    None
}

/// What to put after the cursor when > has just been typed: the closing tag, if what's before the cursor ends an
/// opening tag that has none yet on the line, and isn't one of HTML's empty elements.
pub fn closing_for(before: &str, after: &str, html: bool) -> Option<String> {
    if !before.ends_with('>') || before.ends_with("/>") {
        return None;
    }
    let tag = tags_in(before, 0).into_iter().last()?;
    let chars: Vec<char> = before.chars().collect();
    // The tag that ends at the cursor.
    let closes_here = chars[tag.end..].iter().collect::<String>().find('>').map(|i| tag.end + i + 1) == Some(chars.len());
    if !closes_here || tag.closing || (html && VOID.contains(&tag.name.to_ascii_lowercase().as_str())) {
        return None;
    }
    let close = format!("</{}>", tag.name);
    if after.trim_start().starts_with(&close) {
        return None;
    }
    Some(close)
}

/// Whether the `<` at character `at` of a line starts JSX rather than a comparison or a type's generics: what's
/// before it is the start of a line or something an expression can follow (`(`, `=>`, `return`, `&&`, `?`...), and
/// what follows is a component's name (a capital, more than one letter) or one of HTML's tags, or a fragment's `>`.
fn jsx_start(chars: &[char], at: usize) -> bool {
    let before: String = chars[..at].iter().collect();
    let before = before.trim_end();
    let after_expression = before.is_empty() || before.ends_with(['(', '>', '{', ',', '=', ':', '?', '&', '|', ';', '[']) || before.ends_with("return") || before.ends_with("=>");
    if !after_expression {
        return false;
    }
    let name: String = chars[at + 1..].iter().take_while(|c| is_name_char(**c)).collect();
    match name.chars().next() {
        None => chars.get(at + 1) == Some(&'>'),
        Some(first) if first.is_uppercase() => name.chars().count() > 1,
        Some(_) => crate::emmet::is_tag(&name),
    }
}

/// Whether what's before the cursor ends an opening JSX tag (`<div className="a">`, `<Card>`, `<>`): not a closing
/// or self-closing one, and not a generic type such as `Array<string>` or a comparison.
pub fn jsx_opening_at_end(before: &str) -> bool {
    let chars: Vec<char> = before.chars().collect();
    if chars.last() != Some(&'>') || before.ends_with("/>") || before.ends_with("=>") {
        return false;
    }
    if before.ends_with("<>") {
        return jsx_start(&chars, chars.len() - 2);
    }
    let Some(tag) = tags_in(before, 0).into_iter().last() else { return false };
    let closes_here = chars[tag.end..].iter().collect::<String>().find('>').map(|i| tag.end + i + 1) == Some(chars.len());
    // The `<` is just before the name, or before a `/` for a closing tag.
    let Some(at) = chars[..tag.start].iter().rposition(|c| *c == '<') else { return false };
    closes_here && !tag.closing && jsx_start(&chars, at)
}

/// What to put after the cursor when > has just been typed in JSX: the closing tag of an opening one, or `</>`
/// for a fragment. Generics and comparisons get nothing.
pub fn jsx_closing_for(before: &str, after: &str) -> Option<String> {
    if !jsx_opening_at_end(before) {
        return None;
    }
    let close = if before.ends_with("<>") { "</>".to_string() } else { format!("</{}>", tags_in(before, 0).into_iter().last()?.name) };
    (!after.trim_start().starts_with(&close)).then_some(close)
}

/// What Enter does in JSX: after an opening tag (or `(`, `{`, `[`) the next line goes in a step, and between an
/// opener and its closer the closer goes on a line of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsxEnter {
    Indent,
    Split,
}

pub fn jsx_on_enter(before: &str, after: &str) -> Option<JsxEnter> {
    let before = before.trim_end();
    let rest = after.trim_start();
    if let Some(open) = before.chars().last().filter(|c| matches!(c, '(' | '{' | '[')) {
        let closer = match open {
            '(' => ')',
            '{' => '}',
            _ => ']',
        };
        return Some(if rest.starts_with(closer) { JsxEnter::Split } else { JsxEnter::Indent });
    }
    if jsx_opening_at_end(before) {
        return Some(if rest.starts_with("</") { JsxEnter::Split } else { JsxEnter::Indent });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    #[test]
    fn jsx_tags_are_told_from_generics_and_comparisons() {
        assert!(jsx_opening_at_end("  return <div className=\"a\">"));
        assert!(jsx_opening_at_end("const a = (<Card title={x}>"));
        assert!(jsx_opening_at_end("<Foo.Bar>"));
        assert!(jsx_opening_at_end("<ul>"));
        assert!(jsx_opening_at_end("    <>"));
        assert!(!jsx_opening_at_end("const a: Array<string>"), "a generic");
        assert!(!jsx_opening_at_end("useState<string>"));
        assert!(!jsx_opening_at_end("if (a <b && c>"), "a comparison");
        assert!(!jsx_opening_at_end("<br />"));
        assert!(!jsx_opening_at_end("</div>"));
        assert!(!jsx_opening_at_end("const f = <T>"), "a lone capital is a type");
        assert!(!jsx_opening_at_end("x => y >"));
        assert_eq!(jsx_closing_for("  <section id=\"a\">", "").as_deref(), Some("</section>"));
        assert_eq!(jsx_closing_for("  <>", "").as_deref(), Some("</>"));
        assert_eq!(jsx_closing_for("  <b>", "</b>"), None, "already closed");
        assert_eq!(jsx_closing_for("Array<string>", ""), None);
    }

    #[test]
    fn enter_in_jsx_indents_and_splits() {
        assert_eq!(jsx_on_enter("  return (", ""), Some(JsxEnter::Indent));
        assert_eq!(jsx_on_enter("  <div>", "</div>"), Some(JsxEnter::Split));
        assert_eq!(jsx_on_enter("  <div>", ""), Some(JsxEnter::Indent));
        assert_eq!(jsx_on_enter("f({", "})"), Some(JsxEnter::Split));
        assert_eq!(jsx_on_enter("f({", ""), Some(JsxEnter::Indent));
        assert_eq!(jsx_on_enter("{", "}"), Some(JsxEnter::Split));
        assert_eq!(jsx_on_enter("  <br />", ""), None);
        assert_eq!(jsx_on_enter("const a = 1;", ""), None);
    }

    #[test]
    fn tags_are_found_on_a_line() {
        let found = tags_in(r#"<div class="a>b"><br/><p>x</p></div><!-- <i> -->"#, 0);
        let names: Vec<(&str, bool)> = found.iter().map(|t| (t.name.as_str(), t.closing)).collect();
        assert_eq!(names, [("div", false), ("p", false), ("p", true), ("div", true)]);
        assert_eq!(tag_at("<ul>", 0, 2).unwrap().name, "ul");
    }

    #[test]
    fn partners_are_found_past_nested_tags_of_the_same_name() {
        let doc = lines("<div>\n  <div>inner</div>\n  <p>x</p>\n</div>");
        let line = |n: usize| doc[n].clone();
        let outer = tag_at(&doc[0], 0, 2).unwrap();
        let closing = partner(&outer, doc.len(), &line).unwrap();
        assert_eq!((closing.line, closing.start, closing.closing), (3, 2, true));
        let back = partner(&closing, doc.len(), &line).unwrap();
        assert_eq!((back.line, back.start), (0, 1));
        let inner = tag_at(&doc[1], 1, 4).unwrap();
        assert_eq!(partner(&inner, doc.len(), &line).unwrap().line, 1);
    }

    #[test]
    fn a_closing_tag_goes_in_as_the_opening_one_is_typed() {
        assert_eq!(closing_for("<section id=\"x\">", "", true).as_deref(), Some("</section>"));
        assert_eq!(closing_for("<br>", "", true), None, "an empty element");
        assert_eq!(closing_for("<br>", "", false).as_deref(), Some("</br>"), "XML has none");
        assert_eq!(closing_for("<img/>", "", true), None);
        assert_eq!(closing_for("</p>", "", true), None);
        assert_eq!(closing_for("<p>", "</p>", true), None, "already closed");
        assert_eq!(closing_for("a > b", "", true), None);
    }
}
