//! The regular expression tester, and JSON and YAML checked against a JSON Schema.
//!
//! The tester shows every match of a pattern in some text, where it is, and what each group took, with a
//! replacement worked out beside it. The schema check takes the JSON or YAML in front and a schema, and lists each
//! place the one doesn't keep to the other, by its path. It knows the parts of JSON Schema most schemas use: type,
//! properties, required, additionalProperties, items, enum and const, the number and length limits, pattern,
//! uniqueItems, allOf, anyOf, oneOf and not, and $ref to a definition in the same schema.

use eframe::egui::{self, RichText, Ui};
use serde_json::Value;

// ---------- Matches ----------

/// One match: where it starts and ends (by byte), its line and column (from 1), and each group's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
    pub groups: Vec<(String, Option<String>)>,
}

/// The most matches listed.
const MOST: usize = 2000;

pub fn build(pattern: &str, ignore_case: bool, multi_line: bool, dot_all: bool) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(pattern).case_insensitive(ignore_case).multi_line(multi_line).dot_matches_new_line(dot_all).build().map_err(|e| e.to_string())
}

/// Every match in the text, with its groups named or numbered.
pub fn matches(re: &regex::Regex, text: &str) -> Vec<Found> {
    let names: Vec<String> = re.capture_names().enumerate().skip(1).map(|(n, name)| name.map_or_else(|| n.to_string(), str::to_string)).collect();
    re.captures_iter(text)
        .take(MOST)
        .filter_map(|caps| {
            let whole = caps.get(0)?;
            let before = &text[..whole.start()];
            let line = before.matches('\n').count() + 1;
            let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
            let groups = names.iter().enumerate().map(|(i, name)| (name.clone(), caps.get(i + 1).map(|m| m.as_str().to_string()))).collect();
            Some(Found { start: whole.start(), end: whole.end(), line, column, groups })
        })
        .collect()
}

// ---------- Schemas ----------

/// Reads JSON, or YAML as JSON would have it.
pub fn read_data(text: &str) -> Result<Value, String> {
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        return Ok(value);
    }
    let docs = yaml_rust::YamlLoader::load_from_str(text).map_err(|e| crate::i18n::tf!("It's neither JSON nor YAML that can be read: {e}"))?;
    Ok(docs.first().map_or(Value::Null, yaml_to_json))
}

fn yaml_to_json(yaml: &yaml_rust::Yaml) -> Value {
    use yaml_rust::Yaml;
    match yaml {
        Yaml::Real(text) => text.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map_or(Value::Null, Value::Number),
        Yaml::Integer(n) => Value::from(*n),
        Yaml::String(s) => Value::String(s.clone()),
        Yaml::Boolean(b) => Value::Bool(*b),
        Yaml::Array(items) => Value::Array(items.iter().map(yaml_to_json).collect()),
        Yaml::Hash(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let key = match k {
                        Yaml::String(s) => s.clone(),
                        Yaml::Integer(n) => n.to_string(),
                        Yaml::Real(r) => r.clone(),
                        Yaml::Boolean(b) => b.to_string(),
                        _ => format!("{k:?}"),
                    };
                    (key, yaml_to_json(v))
                })
                .collect(),
        ),
        _ => Value::Null,
    }
}

/// A place the data doesn't keep to the schema: its path (as a JSON pointer) and what's wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub path: String,
    pub words: String,
}

fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() || n.as_f64().is_some_and(|f| f.fract() == 0.0) => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn is_type(value: &Value, wanted: &str) -> bool {
    let has = type_of(value);
    has == wanted || (wanted == "number" && has == "integer")
}

/// Checks data against a schema, listing every problem.
pub fn check(schema: &Value, data: &Value) -> Vec<Problem> {
    let mut problems = Vec::new();
    check_at(schema, schema, data, "", &mut problems, 0);
    problems
}

fn check_at(root: &Value, schema: &Value, data: &Value, path: &str, out: &mut Vec<Problem>, depth: usize) {
    let mut say = |words: String| out.push(Problem { path: if path.is_empty() { "/".into() } else { path.to_string() }, words });
    if depth > 64 {
        return;
    }
    let schema = match schema {
        Value::Bool(true) => return,
        Value::Bool(false) => {
            say(crate::i18n::tr("Nothing is allowed here.").to_string());
            return;
        }
        Value::Object(map) => map,
        _ => return,
    };
    // A reference to a definition in the same schema, such as #/definitions/point or #/$defs/point.
    if let Some(Value::String(reference)) = schema.get("$ref") {
        match reference.strip_prefix('#').and_then(|pointer| root.pointer(pointer)) {
            Some(target) => check_at(root, target, data, path, out, depth + 1),
            None => say(crate::i18n::tf!("The schema's $ref {reference} can't be found; only references within the schema are followed.")),
        }
        return;
    }
    if let Some(wanted) = schema.get("type") {
        let types: Vec<&str> = match wanted {
            Value::String(t) => vec![t.as_str()],
            Value::Array(list) => list.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        if !types.is_empty() && !types.iter().any(|t| is_type(data, t)) {
            say(crate::i18n::tf!("Should be {}, but is {}.", types.join(" or "), type_of(data)));
        }
    }
    if let Some(Value::Array(allowed)) = schema.get("enum")
        && !allowed.contains(data)
    {
        say(crate::i18n::tf!("Should be one of {}.", allowed.iter().map(Value::to_string).collect::<Vec<_>>().join(", ")));
    }
    if let Some(wanted) = schema.get("const")
        && wanted != data
    {
        say(crate::i18n::tf!("Should be {wanted}."));
    }
    if let Some(n) = data.as_f64() {
        let limit = |key: &str| schema.get(key).and_then(Value::as_f64);
        if let Some(least) = limit("minimum").filter(|least| n < *least) {
            say(crate::i18n::tf!("Should be at least {least}."));
        }
        if let Some(most) = limit("maximum").filter(|most| n > *most) {
            say(crate::i18n::tf!("Should be at most {most}."));
        }
        if let Some(least) = limit("exclusiveMinimum").filter(|least| n <= *least) {
            say(crate::i18n::tf!("Should be more than {least}."));
        }
        if let Some(most) = limit("exclusiveMaximum").filter(|most| n >= *most) {
            say(crate::i18n::tf!("Should be less than {most}."));
        }
        if let Some(step) = limit("multipleOf").filter(|step| *step > 0.0 && ((n / step) - (n / step).round()).abs() > 1e-9) {
            say(crate::i18n::tf!("Should be a multiple of {step}."));
        }
    }
    if let Value::String(text) = data {
        let length = text.chars().count() as u64;
        if let Some(least) = schema.get("minLength").and_then(Value::as_u64).filter(|least| length < *least) {
            say(crate::i18n::tf!("Should be at least {least} characters long."));
        }
        if let Some(most) = schema.get("maxLength").and_then(Value::as_u64).filter(|most| length > *most) {
            say(crate::i18n::tf!("Should be at most {most} characters long."));
        }
        if let Some(Value::String(pattern)) = schema.get("pattern") {
            match regex::Regex::new(pattern) {
                Ok(re) if !re.is_match(text) => say(crate::i18n::tf!("Should match the pattern {pattern}.")),
                Err(_) => say(crate::i18n::tf!("The schema's pattern {pattern} isn't one that can be read.")),
                _ => {}
            }
        }
    }
    if let Value::Array(items) = data {
        let count = items.len() as u64;
        if let Some(least) = schema.get("minItems").and_then(Value::as_u64).filter(|least| count < *least) {
            say(crate::i18n::tf!("Should have at least {least} items."));
        }
        if let Some(most) = schema.get("maxItems").and_then(Value::as_u64).filter(|most| count > *most) {
            say(crate::i18n::tf!("Should have at most {most} items."));
        }
        if schema.get("uniqueItems") == Some(&Value::Bool(true)) {
            for (i, item) in items.iter().enumerate() {
                if items[..i].contains(item) {
                    say(crate::i18n::tf!("Item {i} is the same as one before it; each should be different."));
                }
            }
        }
        if let Some(each) = schema.get("items") {
            for (i, item) in items.iter().enumerate() {
                check_at(root, each, item, &format!("{path}/{i}"), out, depth + 1);
            }
        }
    }
    if let Value::Object(fields) = data {
        if let Some(Value::Array(required)) = schema.get("required") {
            for name in required.iter().filter_map(Value::as_str) {
                if !fields.contains_key(name) {
                    out.push(Problem { path: if path.is_empty() { "/".into() } else { path.to_string() }, words: crate::i18n::tf!("{name} is missing, and it's required.") });
                }
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        for (name, value) in fields {
            let at = format!("{path}/{}", name.replace('~', "~0").replace('/', "~1"));
            match properties.and_then(|p| p.get(name)) {
                Some(property) => check_at(root, property, value, &at, out, depth + 1),
                None => match schema.get("additionalProperties") {
                    Some(Value::Bool(false)) => out.push(Problem { path: at, words: crate::i18n::tf!("{name} isn't one of the fields allowed here.") }),
                    Some(more @ Value::Object(_)) => check_at(root, more, value, &at, out, depth + 1),
                    _ => {}
                },
            }
        }
    }
    let passes = |sub: &Value| {
        let mut inner = Vec::new();
        check_at(root, sub, data, path, &mut inner, depth + 1);
        inner
    };
    if let Some(Value::Array(all)) = schema.get("allOf") {
        for sub in all {
            out.extend(passes(sub));
        }
    }
    if let Some(Value::Array(any)) = schema.get("anyOf")
        && !any.iter().any(|sub| passes(sub).is_empty())
    {
        out.push(Problem { path: if path.is_empty() { "/".into() } else { path.to_string() }, words: crate::i18n::tr("Fits none of the choices it should fit one of (anyOf).").to_string() });
    }
    if let Some(Value::Array(one)) = schema.get("oneOf") {
        let fitting = one.iter().filter(|sub| passes(sub).is_empty()).count();
        if fitting != 1 {
            out.push(Problem { path: if path.is_empty() { "/".into() } else { path.to_string() }, words: crate::i18n::tf!("Fits {fitting} of the choices, and should fit exactly one (oneOf).") });
        }
    }
    if let Some(not) = schema.get("not")
        && passes(not).is_empty()
    {
        out.push(Problem { path: if path.is_empty() { "/".into() } else { path.to_string() }, words: crate::i18n::tr("Fits what it mustn't (not).").to_string() });
    }
}

// ---------- The window ----------

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Pattern,
    Schema,
}

#[derive(Default)]
pub struct Tester {
    pub tab: Tab,
    pub pattern: String,
    pub ignore_case: bool,
    pub multi_line: bool,
    pub dot_all: bool,
    pub text: String,
    pub replacement: String,
    /// The schema's file, and what the check last found, or why it couldn't.
    pub schema_path: Option<std::path::PathBuf>,
    pub schema_result: Option<Result<Vec<Problem>, String>>,
}

/// What the window asks the editor for.
pub enum Asked {
    /// Check the file in front against the schema chosen.
    Check,
    /// Choose the schema's file.
    ChooseSchema,
    /// Use the pattern in the Find box.
    ToFind(String),
}

impl Tester {
    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Pattern, crate::i18n::tr("Regular expression"));
            ui.selectable_value(&mut self.tab, Tab::Schema, crate::i18n::tr("JSON or YAML against a schema"));
        });
        ui.separator();
        match self.tab {
            Tab::Pattern => {
                ui.add(egui::TextEdit::singleline(&mut self.pattern).hint_text(crate::i18n::tr("A pattern, such as (\\w+)@(\\w+)\\.com")).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.ignore_case, crate::i18n::tr("Ignore case"));
                    ui.checkbox(&mut self.multi_line, crate::i18n::tr("^ and $ at each line")).on_hover_text(crate::i18n::tr("Multi-line: ^ and $ match at the start and end of every line, not only of the whole text"));
                    ui.checkbox(&mut self.dot_all, crate::i18n::tr(". takes line breaks")).on_hover_text(crate::i18n::tr(". matches a line break too"));
                    if ui.button(crate::i18n::tr("Use in Find")).on_hover_text(crate::i18n::tr("The pattern goes in the Find box, as a regular expression")).clicked() {
                        asked = Some(Asked::ToFind(self.pattern.clone()));
                    }
                });
                ui.label(RichText::new(crate::i18n::tr("Text to try it on:")).small().color(crate::theme::text_dim()));
                egui::ScrollArea::vertical().id_salt("regex-text").max_height(140.0).show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut self.text).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).desired_rows(5));
                });
                if self.pattern.is_empty() {
                    return asked;
                }
                let re = match build(&self.pattern, self.ignore_case, self.multi_line, self.dot_all) {
                    Ok(re) => re,
                    Err(e) => {
                        ui.colored_label(crate::theme::rainbow_mistake(), e);
                        return asked;
                    }
                };
                let found = matches(&re, &self.text);
                ui.label(RichText::new(match found.len() {
                    0 => crate::i18n::tr("No matches.").to_string(),
                    1 => crate::i18n::tr("1 match:").to_string(),
                    n => crate::i18n::tf!("{n} matches:"),
                }).strong());
                // The text with each match marked.
                let mut job = egui::text::LayoutJob::default();
                let font = egui::FontId::monospace(12.5);
                let mut at = 0;
                for found in found.iter().take(400) {
                    job.append(&self.text[at..found.start], 0.0, egui::TextFormat::simple(font.clone(), crate::theme::text()));
                    let mut marked = egui::TextFormat::simple(font.clone(), crate::theme::text());
                    marked.background = ui.visuals().selection.bg_fill;
                    job.append(&self.text[found.start..found.end], 0.0, marked);
                    at = found.end;
                }
                job.append(&self.text[at..], 0.0, egui::TextFormat::simple(font, crate::theme::text()));
                egui::ScrollArea::vertical().id_salt("regex-marked").max_height(120.0).show(ui, |ui| {
                    ui.add(egui::Label::new(job).wrap());
                });
                egui::ScrollArea::vertical().id_salt("regex-found").max_height(160.0).show(ui, |ui| {
                    egui::Grid::new("regex-found-grid").num_columns(3).striped(true).show(ui, |ui| {
                        for (n, found) in found.iter().enumerate() {
                            ui.label(RichText::new(format!("{}", n + 1)).color(crate::theme::text_faint()));
                            ui.label(crate::i18n::tf!("line {}, column {}", found.line, found.column));
                            let groups: Vec<String> = found.groups.iter().map(|(name, value)| format!("{name}: {}", value.as_deref().map_or_else(|| crate::i18n::tr("(none)").to_string(), |v| format!("\u{201C}{v}\u{201D}")))).collect();
                            ui.label(RichText::new(format!("\u{201C}{}\u{201D}  {}", &self.text[found.start..found.end], groups.join("  "))).monospace());
                            ui.end_row();
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Replace with"));
                    ui.add(egui::TextEdit::singleline(&mut self.replacement).hint_text(crate::i18n::tr("$1, ${name}")).font(egui::TextStyle::Monospace).desired_width(220.0));
                });
                if !self.replacement.is_empty() {
                    let replaced = re.replace_all(&self.text, self.replacement.as_str());
                    egui::ScrollArea::vertical().id_salt("regex-replaced").max_height(100.0).show(ui, |ui| {
                        ui.label(RichText::new(replaced.as_ref()).monospace());
                    });
                }
            }
            Tab::Schema => {
                ui.label(crate::i18n::tr("The JSON or YAML in front is checked against a JSON Schema."));
                ui.horizontal(|ui| {
                    let name = self.schema_path.as_ref().and_then(|p| p.file_name()).map_or_else(|| crate::i18n::tr("No schema chosen").to_string(), |n| n.to_string_lossy().into_owned());
                    ui.label(RichText::new(name).strong());
                    if ui.button(crate::i18n::tr("Choose the schema\u{2026}")).clicked() {
                        asked = Some(Asked::ChooseSchema);
                    }
                    if ui.add_enabled(self.schema_path.is_some(), egui::Button::new(crate::i18n::tr("Check"))).clicked() {
                        asked = Some(Asked::Check);
                    }
                });
                match &self.schema_result {
                    Some(Ok(problems)) if problems.is_empty() => {
                        ui.colored_label(crate::theme::accent(), crate::i18n::tr("It keeps to the schema."));
                    }
                    Some(Ok(problems)) => {
                        ui.label(RichText::new(crate::i18n::tf!("{} places don't keep to the schema:", problems.len())).strong());
                        egui::ScrollArea::vertical().id_salt("schema-problems").max_height(260.0).show(ui, |ui| {
                            egui::Grid::new("schema-grid").num_columns(2).striped(true).show(ui, |ui| {
                                for problem in problems {
                                    ui.label(RichText::new(&problem.path).monospace());
                                    ui.label(&problem.words);
                                    ui.end_row();
                                }
                            });
                        });
                    }
                    Some(Err(why)) => {
                        ui.colored_label(crate::theme::rainbow_mistake(), why);
                    }
                    None => {}
                }
                ui.label(RichText::new(crate::i18n::tr("Types, properties and required fields, items, enums, limits on numbers and lengths, patterns, allOf, anyOf, oneOf, not, and $ref within the schema are checked.")).small().color(crate::theme::text_dim()));
            }
        }
        asked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_show_where_they_are_and_their_groups() {
        let re = build(r"(?P<user>\w+)@(\w+)\.com", false, false, false).unwrap();
        let found = matches(&re, "write to\njo@example.com or al@test.com");
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].line, found[0].column), (2, 1));
        assert_eq!(found[0].groups, [("user".to_string(), Some("jo".to_string())), ("2".to_string(), Some("example".to_string()))]);
        assert_eq!((found[1].line, found[1].column), (2, 19));
        assert!(build("(", false, false, false).is_err());
    }

    #[test]
    fn data_is_checked_against_its_schema() {
        let schema = json!({
            "type": "object",
            "required": ["name", "size"],
            "additionalProperties": false,
            "properties": {
                "name": {"type": "string", "minLength": 2, "pattern": "^[a-z]+$"},
                "size": {"$ref": "#/$defs/size"},
                "tags": {"type": "array", "items": {"enum": ["a", "b"]}, "uniqueItems": true},
                "kind": {"oneOf": [{"const": "x"}, {"type": "integer"}]}
            },
            "$defs": {"size": {"type": "integer", "minimum": 1, "maximum": 10}}
        });
        assert!(check(&schema, &json!({"name": "box", "size": 3, "tags": ["a"], "kind": 2})).is_empty());
        let problems = check(&schema, &json!({"name": "B", "size": 30, "tags": ["a", "c", "a"], "kind": true, "extra": 1}));
        let paths: Vec<&str> = problems.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(paths, ["/name", "/name", "/size", "/tags", "/tags/1", "/kind", "/extra"], "{problems:?}");
        let missing = check(&schema, &json!({}));
        assert_eq!(missing.len(), 2);
    }

    #[test]
    fn yaml_is_read_as_json_would_have_it() {
        let value = read_data("name: box\nsize: 3\nscale: 1.5\ntags:\n  - a\n  - b\n").unwrap();
        assert_eq!(value, json!({"name": "box", "size": 3, "scale": 1.5, "tags": ["a", "b"]}));
        assert!(read_data("{\"a\": [1, 2]}").is_ok());
        assert!(read_data("a: [unclosed").is_err());
    }
}
