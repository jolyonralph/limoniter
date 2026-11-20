//! The file formats VS Code extensions are written in, read into what Limoniter uses: JSON with comments (as
//! VS Code allows in its own files), Apple's property lists (the XML that older TextMate grammars and themes are),
//! and TextMate grammars turned into the Sublime Text grammars that syntect colours with.
//!
//! A TextMate grammar and a Sublime one describe the same thing in different words: a list of patterns, each a
//! regular expression with the scope it gives, some of which start a stretch (a string, a comment) that runs until
//! another pattern ends it. TextMate's begin and end become a match that pushes a context of its own, with the end
//! as the first pattern in it that pops back out. A few things have no Sublime equal and are approximated, and a
//! pattern whose regular expression can't be used is left out rather than spoiling the rest.

use serde_json::{Map, Value};
use std::collections::HashMap;

// ---------- JSON with comments ----------

/// Reads JSON as VS Code writes its own files: with // and /* */ comments, and commas left after the last item.
pub fn parse_jsonc(text: &str) -> Result<Value, String> {
    let text = text.trim_start_matches('\u{feff}');
    if let Ok(value) = serde_json::from_str(text) {
        return Ok(value);
    }
    let cleaned = strip_trailing_commas(&strip_comments(text));
    serde_json::from_str(&cleaned).map_err(|e| crate::i18n::tf!("it isn't JSON that can be read: {e}"))
}

/// The text with its comments taken out, and what's inside strings left alone.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for skipped in chars.by_ref() {
                    if skipped == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut last = ' ';
                for skipped in chars.by_ref() {
                    if last == '*' && skipped == '/' {
                        break;
                    }
                    if skipped == '\n' {
                        out.push('\n');
                    }
                    last = skipped;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

/// The text with any comma that comes just before a closing bracket taken out.
fn strip_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if c == ',' && chars[i + 1..].iter().find(|c| !c.is_whitespace()).is_some_and(|c| matches!(c, '}' | ']')) {
            // Left out.
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

// ---------- Property lists ----------

/// Reads an Apple property list (XML), as .tmLanguage grammars and .tmTheme themes are written, into JSON's terms.
pub fn plist_to_json(text: &str) -> Result<Value, String> {
    let options = roxmltree::ParsingOptions { allow_dtd: true, ..roxmltree::ParsingOptions::default() };
    let doc = roxmltree::Document::parse_with_options(text.trim_start_matches('\u{feff}'), options).map_err(|e| crate::i18n::tf!("it isn't XML that can be read: {e}"))?;
    let root = doc.root_element();
    let top = if root.has_tag_name("plist") { root.children().find(roxmltree::Node::is_element) } else { Some(root) };
    top.map(plist_value).ok_or_else(|| crate::i18n::tr("the property list is empty").to_string())
}

fn plist_value(node: roxmltree::Node<'_, '_>) -> Value {
    let text = || node.descendants().filter(roxmltree::Node::is_text).filter_map(|n| n.text()).collect::<String>();
    match node.tag_name().name() {
        "dict" => {
            let mut map = Map::new();
            let mut key: Option<String> = None;
            for child in node.children().filter(roxmltree::Node::is_element) {
                if child.has_tag_name("key") {
                    key = Some(child.descendants().filter(roxmltree::Node::is_text).filter_map(|n| n.text()).collect());
                } else if let Some(name) = key.take() {
                    map.insert(name, plist_value(child));
                }
            }
            Value::Object(map)
        }
        "array" => Value::Array(node.children().filter(roxmltree::Node::is_element).map(plist_value).collect()),
        "integer" => text().trim().parse::<i64>().map_or(Value::Null, Value::from),
        "real" => text().trim().parse::<f64>().ok().and_then(serde_json::Number::from_f64).map_or(Value::Null, Value::Number),
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => Value::String(text()),
    }
}

/// Reads a grammar or theme file by its ending: a property list for .tmLanguage, .tmTheme and .plist, YAML for
/// .yaml-tmlanguage, and JSON (with comments) for anything else.
pub fn read_structured(path: &std::path::Path, text: &str) -> Result<Value, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if name.ends_with(".tmlanguage") || name.ends_with(".tmtheme") || name.ends_with(".plist") || (!name.ends_with(".json") && text.trim_start_matches('\u{feff}').trim_start().starts_with('<')) {
        plist_to_json(text)
    } else if name.ends_with(".yaml-tmlanguage") || name.ends_with(".yaml") || name.ends_with(".yml") {
        let docs = yaml_rust::YamlLoader::load_from_str(text).map_err(|e| crate::i18n::tf!("it isn't YAML that can be read: {e}"))?;
        docs.first().map(yaml_to_json).ok_or_else(|| crate::i18n::tr("it's empty").to_string())
    } else {
        parse_jsonc(text)
    }
}

fn yaml_to_json(yaml: &yaml_rust::Yaml) -> Value {
    use yaml_rust::Yaml;
    match yaml {
        Yaml::String(s) | Yaml::Real(s) => Value::String(s.clone()),
        Yaml::Integer(i) => Value::from(*i),
        Yaml::Boolean(b) => Value::Bool(*b),
        Yaml::Array(items) => Value::Array(items.iter().map(yaml_to_json).collect()),
        Yaml::Hash(map) => Value::Object(
            map.iter()
                .filter_map(|(k, v)| {
                    let key = match k {
                        Yaml::String(s) | Yaml::Real(s) => s.clone(),
                        Yaml::Integer(i) => i.to_string(),
                        _ => return None,
                    };
                    Some((key, yaml_to_json(v)))
                })
                .collect(),
        ),
        _ => Value::Null,
    }
}

// ---------- Writing YAML ----------

/// A little YAML, written in its flow style, which is JSON's: enough for a grammar, and with every string in double
/// quotes, so that nothing in a regular expression can be taken for YAML's own punctuation.
enum Yaml {
    Text(String),
    Number(i64),
    Bool(bool),
    List(Vec<Yaml>),
    Map(Vec<(Yaml, Yaml)>),
}

impl Yaml {
    fn map(pairs: Vec<(&str, Yaml)>) -> Self {
        Yaml::Map(pairs.into_iter().map(|(k, v)| (Yaml::Text(k.to_string()), v)).collect())
    }

    fn text(s: &str) -> Self {
        Yaml::Text(s.to_string())
    }

    fn write(&self, out: &mut String) {
        match self {
            Yaml::Text(s) => out.push_str(&serde_json::to_string(s).unwrap_or_default()),
            Yaml::Number(n) => out.push_str(&n.to_string()),
            Yaml::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Yaml::List(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Yaml::Map(pairs) => {
                out.push('{');
                for (i, (key, value)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    key.write(out);
                    out.push_str(": ");
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

// ---------- TextMate grammars ----------

/// A TextMate grammar as a Sublime grammar: the YAML syntect reads, and how many patterns had to be left out.
#[derive(Debug)]
pub struct Converted {
    pub yaml: String,
    pub scope: String,
    pub left_out: usize,
}

/// What a grammar is to be called and which files it's for, from the extension's description of the language.
pub struct GrammarInfo<'a> {
    pub name: &'a str,
    pub endings: &'a [String],
    pub first_line: Option<&'a str>,
}

/// Turns a TextMate grammar into a Sublime one.
pub fn convert(grammar: &Value, info: &GrammarInfo<'_>) -> Result<Converted, String> {
    let scope = grammar.get("scopeName").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).ok_or(crate::i18n::tr("the grammar has no scopeName"))?.trim().to_string();
    let mut converter = Converter::default();
    let main = converter.with_repository(grammar.get("repository"), "", &mut |c| c.patterns(grammar.get("patterns")));
    let mut contexts = vec![(Yaml::text("main"), Yaml::List(main))];
    for (name, entries) in converter.contexts.drain(..) {
        contexts.push((Yaml::Text(name), Yaml::List(entries)));
    }
    let mut endings: Vec<Yaml> = info.endings.iter().map(|e| Yaml::text(e.trim_start_matches('.'))).collect();
    if endings.is_empty()
        && let Some(types) = grammar.get("fileTypes").and_then(Value::as_array)
    {
        endings = types.iter().filter_map(Value::as_str).map(|e| Yaml::text(e.trim_start_matches('.'))).collect();
    }
    let mut top = vec![("name", Yaml::text(info.name)), ("scope", Yaml::text(&clean_scope(&scope))), ("file_extensions", Yaml::List(endings))];
    let first_line = info.first_line.or_else(|| grammar.get("firstLineMatch").and_then(Value::as_str)).map(fix_regex).filter(|r| usable(r));
    if let Some(first_line) = first_line {
        top.push(("first_line_match", Yaml::Text(first_line)));
    }
    top.push(("contexts", Yaml::Map(contexts)));
    let mut yaml = String::new();
    Yaml::map(top).write(&mut yaml);
    Ok(Converted { yaml, scope, left_out: converter.left_out })
}

#[derive(Default)]
struct Converter {
    /// Contexts made so far, beyond main, in the order they were made.
    contexts: Vec<(String, Vec<Yaml>)>,
    /// The repositories in reach, innermost last: each rule's name and the context it became.
    scopes: Vec<HashMap<String, String>>,
    made: usize,
    left_out: usize,
    depth: usize,
}

/// Context names are kept to letters, digits, - and _, which syntect never reads as anything else.
fn context_name(key: &str) -> String {
    let name: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if name == "main" || name == "prototype" || name.is_empty() { format!("{name}_") } else { name }
}

impl Converter {
    fn new_name(&mut self, hint: &str) -> String {
        self.made += 1;
        format!("__{}_{}", self.made, context_name(hint).trim_end_matches('_'))
    }

    /// Runs `inside` with a repository's rules in reach, each made into a context of its own first.
    fn with_repository(&mut self, repository: Option<&Value>, prefix: &str, inside: &mut dyn FnMut(&mut Self) -> Vec<Yaml>) -> Vec<Yaml> {
        let Some(rules) = repository.and_then(Value::as_object) else {
            return inside(self);
        };
        let names: HashMap<String, String> = rules
            .keys()
            .map(|key| {
                // The outermost repository's rules keep their own names, so another grammar can include one by it.
                let name = if prefix.is_empty() && self.scopes.is_empty() { context_name(key) } else { self.new_name(key) };
                (key.clone(), name)
            })
            .collect();
        self.scopes.push(names.clone());
        for (key, rule) in rules {
            let entries = if rule.get("match").is_none() && rule.get("begin").is_none() && rule.get("include").is_none() {
                self.with_repository(rule.get("repository"), key, &mut |c| c.patterns(rule.get("patterns")))
            } else {
                self.rule(rule)
            };
            self.contexts.push((names[key].clone(), entries));
        }
        let result = inside(self);
        self.scopes.pop();
        result
    }

    fn patterns(&mut self, patterns: Option<&Value>) -> Vec<Yaml> {
        patterns.and_then(Value::as_array).map(|list| list.iter().flat_map(|rule| self.rule(rule)).collect()).unwrap_or_default()
    }

    /// One TextMate rule as the entries it becomes in a context.
    fn rule(&mut self, rule: &Value) -> Vec<Yaml> {
        if !rule.is_object() || rule.get("disabled").is_some_and(|d| d.as_i64() == Some(1) || d.as_bool() == Some(true)) {
            return Vec::new();
        }
        if self.depth > 64 {
            self.left_out += 1;
            return Vec::new();
        }
        self.depth += 1;
        let entries = self.with_repository(rule.get("repository"), "rule", &mut |c| c.rule_itself(rule));
        self.depth -= 1;
        entries
    }

    fn rule_itself(&mut self, rule: &Value) -> Vec<Yaml> {
        let text = |key: &str| rule.get(key).and_then(Value::as_str);
        if let Some(include) = text("include") {
            return self.include(include).into_iter().collect();
        }
        if let Some(pattern) = text("match") {
            let regex = fix_regex(pattern);
            if !usable(&regex) {
                self.left_out += 1;
                return Vec::new();
            }
            let mut entry = vec![("match", Yaml::Text(regex))];
            if let Some(name) = text("name").map(clean_scope).filter(|s| !s.is_empty()) {
                entry.push(("scope", Yaml::Text(name)));
            }
            if let Some(captures) = captures(rule.get("captures")) {
                entry.push(("captures", captures));
            }
            return vec![Yaml::map(entry)];
        }
        if let Some(begin) = text("begin") {
            let begin = fix_regex(begin);
            let close = text("end").map(|end| (fix_regex(end), false)).or_else(|| text("while").map(|w| (fix_regex(w), true)));
            let Some((close, is_while)) = close.filter(|(close, _)| usable(&begin) && usable(close)) else {
                self.left_out += 1;
                return Vec::new();
            };
            let name = self.new_name(text("name").unwrap_or("block"));
            let mut entries = Vec::new();
            if let Some(scope) = text("name").map(clean_scope).filter(|s| !s.is_empty()) {
                entries.push(Yaml::map(vec![("meta_scope", Yaml::Text(scope))]));
            }
            if let Some(scope) = text("contentName").map(clean_scope).filter(|s| !s.is_empty()) {
                entries.push(Yaml::map(vec![("meta_content_scope", Yaml::Text(scope))]));
            }
            let end_captures = captures(rule.get(if is_while { "whileCaptures" } else { "endCaptures" }).or_else(|| rule.get("captures")));
            let mut ending = Vec::new();
            if is_while {
                // A while carries the stretch on for as long as each new line starts with it, which Sublime
                // grammars can't say; at the start of a line, the while is matched if it's there, and if it isn't
                // the stretch ends.
                let mut keep = vec![("match", Yaml::Text(format!("^(?:{close})")))];
                if let Some(captures) = end_captures {
                    keep.push(("captures", captures));
                }
                ending.push(Yaml::map(keep));
                ending.push(Yaml::map(vec![("match", Yaml::Text(format!("^(?!(?:{close}))"))), ("pop", Yaml::Bool(true))]));
            } else {
                let mut end = vec![("match", Yaml::Text(close)), ("pop", Yaml::Bool(true))];
                if let Some(captures) = end_captures {
                    end.push(("captures", captures));
                }
                ending.push(Yaml::map(end));
            }
            let last = rule.get("applyEndPatternLast").is_some_and(|v| v.as_i64() == Some(1) || v.as_bool() == Some(true)) && !is_while;
            if !last {
                entries.append(&mut ending);
            }
            entries.extend(self.patterns(rule.get("patterns")));
            if last {
                entries.append(&mut ending);
            }
            self.contexts.push((name.clone(), entries));
            let mut push = vec![("match", Yaml::Text(begin)), ("push", Yaml::Text(name))];
            if let Some(captures) = captures(rule.get("beginCaptures").or_else(|| rule.get("captures"))) {
                push.push(("captures", captures));
            }
            return vec![Yaml::map(push)];
        }
        if rule.get("patterns").is_some() {
            // A group of patterns: a context of their own, included where the group was.
            let entries = self.patterns(rule.get("patterns"));
            let name = self.new_name("group");
            self.contexts.push((name.clone(), entries));
            return vec![Yaml::map(vec![("include", Yaml::Text(name))])];
        }
        Vec::new()
    }

    /// An include: a rule from a repository in reach, the grammar itself, or another grammar by its scope.
    fn include(&mut self, include: &str) -> Option<Yaml> {
        let target = if let Some(key) = include.strip_prefix('#') {
            match self.scopes.iter().rev().find_map(|names| names.get(key)) {
                Some(name) => name.clone(),
                None => {
                    self.left_out += 1;
                    return None;
                }
            }
        } else if include == "$self" || include == "$base" {
            "main".to_string()
        } else {
            // Another grammar: syntect finds it by its scope if it has it, and passes over it if it hasn't.
            match include.split_once('#') {
                Some((scope, key)) => format!("scope:{}#{}", scope.trim(), context_name(key)),
                None => format!("scope:{}", include.trim()),
            }
        };
        Some(Yaml::map(vec![("include", Yaml::Text(target))]))
    }
}

/// A rule's captures as Sublime has them: each group's number and scope. Patterns inside a capture are left out.
fn captures(captures: Option<&Value>) -> Option<Yaml> {
    let list: Vec<(Yaml, Yaml)> = match captures? {
        Value::Object(map) => map.iter().filter_map(|(k, v)| Some((k.parse::<i64>().ok()?, v))).collect::<Vec<_>>(),
        Value::Array(items) => items.iter().enumerate().map(|(i, v)| (i as i64, v)).collect(),
        _ => return None,
    }
    .into_iter()
    .filter_map(|(group, capture)| {
        let scope = clean_scope(capture.get("name")?.as_str()?);
        (!scope.is_empty()).then_some((Yaml::Number(group), Yaml::Text(scope)))
    })
    .collect();
    (!list.is_empty()).then_some(Yaml::Map(list))
}

/// A scope name as syntect will take it: TextMate's $1 and the like, which put captured text into a name, left
/// out; no empty parts; and no more than eight parts to each scope.
pub fn clean_scope(scope: &str) -> String {
    scope
        .split_whitespace()
        .filter_map(|one| {
            let atoms: Vec<&str> = one.split('.').filter(|a| !a.is_empty() && !a.contains('$') && a.chars().all(|c| !c.is_control())).take(8).collect();
            (!atoms.is_empty()).then(|| atoms.join("."))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether a regular expression can be used, with its back references (to a begin's captures) stood in for.
fn usable(regex: &str) -> bool {
    let mut stood_in = String::with_capacity(regex.len());
    let mut chars = regex.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some(d) if d.is_ascii_digit() && *d != '0' => {
                    chars.next();
                    stood_in.push('x');
                }
                Some(_) => {
                    stood_in.push(c);
                    stood_in.push(chars.next().unwrap_or(' '));
                }
                None => stood_in.push(c),
            }
        } else {
            stood_in.push(c);
        }
    }
    syntect::parsing::Regex::try_compile(&stood_in).is_none()
}

/// Oniguruma's regular expressions, as TextMate grammars are written in, put into the ones syntect understands
/// where they differ: \h for a hex digit, \e for escape, {,n} for up to n, and \G and \Z.
pub fn fix_regex(regex: &str) -> String {
    let chars: Vec<char> = regex.chars().collect();
    let mut out = String::with_capacity(regex.len() + 8);
    let mut in_class = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            let next = chars[i + 1];
            match next {
                'h' => out.push_str(if in_class { "0-9a-fA-F" } else { "[0-9a-fA-F]" }),
                'H' if !in_class => out.push_str("[^0-9a-fA-F]"),
                'e' => out.push_str("\\x1B"),
                // Where the last match ended: in a grammar, nearly always where this pattern is tried anyway.
                'G' if !in_class => {}
                'Z' if !in_class => out.push_str("(?=\\n?\\z)"),
                _ => {
                    out.push(c);
                    out.push(next);
                }
            }
            i += 2;
            continue;
        }
        if in_class {
            if c == ']' {
                in_class = false;
            } else if c == '[' && chars.get(i + 1) == Some(&':') {
                // A POSIX class, [:alpha:], inside a set.
                if let Some(end) = (i + 2..chars.len().saturating_sub(1)).find(|&j| chars[j] == ':' && chars[j + 1] == ']') {
                    out.extend(&chars[i..end + 2]);
                    i = end + 2;
                    continue;
                }
            }
            out.push(c);
            i += 1;
            continue;
        }
        match c {
            '[' => {
                in_class = true;
                out.push(c);
                // A ] straight after the [ (or [^) is a character in the set, not its end.
                if chars.get(i + 1) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
                if chars.get(i + 1) == Some(&']') {
                    out.push_str("\\]");
                    i += 1;
                }
            }
            '{' if chars.get(i + 1) == Some(&',') && chars[i + 2..].iter().take_while(|c| c.is_ascii_digit()).count() > 0 => out.push_str("{0"),
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntect::parsing::{SyntaxDefinition, SyntaxSetBuilder};

    #[test]
    fn json_with_comments_and_trailing_commas_is_read() {
        let text = "\u{feff}{\n  // a comment\n  \"a\": \"http://x\", /* another */\n  \"b\": [1, 2,],\n}\n";
        let value = parse_jsonc(text).unwrap();
        assert_eq!(value["a"], "http://x", "// inside a string stays");
        assert_eq!(value["b"], serde_json::json!([1, 2]));
        assert!(parse_jsonc("{ nope").is_err());
    }

    #[test]
    fn a_property_list_reads_as_json() {
        let text = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>name</key><string>Thing &amp; more</string>
  <key>fileTypes</key><array><string>thg</string></array>
  <key>count</key><integer>3</integer>
  <key>on</key><true/>
</dict></plist>"#;
        let value = plist_to_json(text).unwrap();
        assert_eq!(value["name"], "Thing & more");
        assert_eq!(value["fileTypes"][0], "thg");
        assert_eq!(value["count"], 3);
        assert_eq!(value["on"], true);
    }

    #[test]
    fn oniguruma_s_own_escapes_are_put_into_syntect_s_terms() {
        assert_eq!(fix_regex(r"0x\h+"), "0x[0-9a-fA-F]+");
        assert_eq!(fix_regex(r"[\h_]"), "[0-9a-fA-F_]");
        assert_eq!(fix_regex(r"\Gfoo"), "foo");
        assert_eq!(fix_regex(r"a{,3}"), "a{0,3}");
        assert_eq!(fix_regex(r"[]a]"), r"[\]a]");
        assert_eq!(fix_regex(r"[[:alpha:]_]\w"), r"[[:alpha:]_]\w");
        assert!(usable(&fix_regex(r"(['\x22])(?:\\.|(?!\1).)*\1")), "back references can be used");
        assert!(!usable("(unclosed"));
    }

    #[test]
    fn scopes_are_made_safe_for_syntect() {
        assert_eq!(clean_scope("keyword.control.$1.ruby  string.quoted"), "keyword.control.ruby string.quoted");
        assert_eq!(clean_scope("a.b.c.d.e.f.g.h.i.j"), "a.b.c.d.e.f.g.h");
        assert_eq!(clean_scope("..."), "");
    }

    fn sample_grammar() -> Value {
        serde_json::json!({
            "scopeName": "source.demo",
            "fileTypes": ["dmo"],
            "patterns": [
                { "include": "#comments" },
                { "match": "\\b(if|then|else)\\b", "name": "keyword.control.demo" },
                { "begin": "\"", "end": "\"", "name": "string.quoted.double.demo", "patterns": [{ "match": "\\\\.", "name": "constant.character.escape.demo" }] },
                { "begin": "(<<)(\\w+)", "end": "^\\2$", "name": "string.unquoted.heredoc.demo", "beginCaptures": { "1": { "name": "keyword.operator.heredoc.demo" } } },
                { "begin": "^>", "while": "^>", "name": "markup.quote.demo" },
                { "match": "(\\w+)\\s*(=)", "captures": { "1": { "name": "variable.other.demo" }, "2": { "name": "keyword.operator.$2.demo" } } },
                { "match": "(?<!", "name": "invalid.broken" },
                { "include": "#missing" },
                { "include": "source.js" },
                { "patterns": [{ "match": "\\d+", "name": "constant.numeric.demo" }] }
            ],
            "repository": {
                "comments": {
                    "patterns": [
                        { "begin": "/\\*", "end": "\\*/", "name": "comment.block.demo" },
                        { "match": "//.*$", "name": "comment.line.double-slash.demo" }
                    ]
                },
                "main": { "match": "x", "name": "a.b" }
            }
        })
    }

    fn scopes_of(syntax_yaml: &str, line: &str) -> Vec<(String, String)> {
        use syntect::parsing::{ParseState, ScopeStack};
        let definition = SyntaxDefinition::load_from_str(syntax_yaml, true, None).unwrap();
        let mut builder = SyntaxSetBuilder::new();
        builder.add(definition);
        let set = builder.build();
        let syntax = set.find_syntax_by_extension("dmo").unwrap();
        let mut state = ParseState::new(syntax);
        let mut stack = ScopeStack::new();
        let mut out = Vec::new();
        for row in line.split_inclusive('\n') {
            let ops = state.parse_line(row, &set).unwrap();
            for (piece, op) in syntect::easy::ScopeRegionIterator::new(&ops, row) {
                stack.apply(op).unwrap();
                if !piece.trim().is_empty() {
                    out.push((piece.to_string(), stack.as_slice().iter().map(|s| s.build_string()).collect::<Vec<_>>().join(" ")));
                }
            }
        }
        out
    }

    #[test]
    fn a_textmate_grammar_colours_as_it_would_in_vs_code() {
        let names = vec!["dmo".to_string()];
        let converted = convert(&sample_grammar(), &GrammarInfo { name: "Demo", endings: &names, first_line: None }).unwrap();
        assert_eq!(converted.scope, "source.demo");
        assert_eq!(converted.left_out, 2, "the broken pattern and the missing rule");
        let found = scopes_of(&converted.yaml, "if a = \"x\\n\" // note\n");
        let scope = |piece: &str| found.iter().find(|(p, _)| p == piece).map(|(_, s)| s.clone()).unwrap_or_else(|| panic!("{piece} not in {found:?}"));
        assert!(scope("if").ends_with("keyword.control.demo"));
        assert!(scope("a").ends_with("variable.other.demo"));
        assert!(scope("=").ends_with("keyword.operator.demo"), "the $2 is left out of the name");
        assert!(scope("\"x").ends_with("string.quoted.double.demo"), "the quote and what's in it are one stretch");
        assert!(scope("\\n").ends_with("constant.character.escape.demo"));
        assert!(scope("// note").ends_with("comment.line.double-slash.demo"), "the repository's rule is included");
        // A begin's captures are used by its end.
        let heredoc = scopes_of(&converted.yaml, "<<END\nstill in it\nEND\nif\n");
        assert!(heredoc.iter().any(|(p, s)| p == "still in it\n" && s.contains("heredoc")), "{heredoc:?}");
        assert!(heredoc.iter().any(|(p, s)| p == "if" && s.ends_with("keyword.control.demo")), "the heredoc ended at END: {heredoc:?}");
        // A while goes on while each line starts with it.
        let quote = scopes_of(&converted.yaml, "> one\n> two\nthree 12\n");
        assert!(quote.iter().any(|(p, s)| p.contains("two") && s.contains("markup.quote.demo")), "{quote:?}");
        assert!(quote.iter().any(|(p, s)| p.contains("three") && !s.contains("markup.quote")), "{quote:?}");
        assert!(quote.iter().any(|(p, s)| p == "12" && s.ends_with("constant.numeric.demo")), "a group of patterns: {quote:?}");
    }

    #[test]
    fn nasty_regular_expressions_survive_being_written_as_yaml() {
        let nasty = ["- [x]: #y", "'\"\\t\u{1}", "{a}, [b]", "@`|>!%&*?", "  spaces  "];
        let rule: Vec<Value> = nasty.iter().map(|n| serde_json::json!({"match": regex_quote(n), "name": "x"})).collect();
        let grammar = serde_json::json!({"scopeName": "source.n", "patterns": rule});
        let converted = convert(&grammar, &GrammarInfo { name: "N", endings: &[], first_line: None }).unwrap();
        let definition = SyntaxDefinition::load_from_str(&converted.yaml, true, None).unwrap();
        assert_eq!(definition.name, "N");
        assert_eq!(definition.contexts["main"].patterns.len(), nasty.len());
    }

    fn regex_quote(text: &str) -> String {
        text.chars().map(|c| if c.is_ascii_punctuation() { format!("\\{c}") } else { c.to_string() }).collect()
    }
}
