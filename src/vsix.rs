//! VS Code extensions, the parts of them that are written down rather than programmed: a .vsix file (a zip, from
//! a file or from Open VSX, the open marketplace VSCodium uses) unpacked into the extensions folder beside the
//! settings, and what its package.json says it gives taken in. Colour themes become document and window themes;
//! TextMate grammars become languages to colour as, with their file endings; snippets go into the Snippets box;
//! file icon themes draw the files' icons; a language's configuration gives its comment marks, the brackets that
//! close themselves and when lines indent. A language server the extension starts is offered as Limoniter's own
//! are, where it can be found: a program the extension carries, a Node.js server in it, or one it's known to use.
//!
//! An extension's own program (its JavaScript) never runs. One that is nothing but its program isn't installed,
//! and says why; one with parts that can be taken in says which of its features are left behind.

use crate::tools::CommentMarks;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// The most a .vsix may unpack to: the largest real ones, which carry their own language server, are a few
/// hundred megabytes.
const MOST_UNPACKED: u64 = 1024 * 1024 * 1024;
const MOST_DOWNLOAD: u64 = 400 * 1024 * 1024;
pub const OPEN_VSX: &str = "https://open-vsx.org";

/// Where extensions are kept, each unpacked in a folder named after it.
pub fn folder() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("LIMONITER_EXTENSIONS") {
        return Some(folder.into());
    }
    // Tests never pick up extensions someone has installed on the machine running them.
    if cfg!(test) {
        return None;
    }
    crate::settings::data_folder().map(|d| d.join("extensions"))
}

// ---------- What an extension says it gives ----------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LanguageEntry {
    pub id: String,
    /// The name VS Code shows it by, its first alias.
    pub name: Option<String>,
    pub extensions: Vec<String>,
    pub filenames: Vec<String>,
    pub first_line: Option<String>,
    pub configuration: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GrammarEntry {
    pub language: Option<String>,
    pub scope: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThemeEntry {
    pub label: String,
    pub ui_theme: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct IconThemeEntry {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnippetEntry {
    pub language: Option<String>,
    pub path: PathBuf,
}

/// An extension's package.json, as far as Limoniter reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Manifest {
    /// publisher.name, in small letters, as VS Code and Open VSX know it.
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
    pub description: String,
    /// Whether it has a program of its own (main or browser), which isn't run here.
    pub has_program: bool,
    pub uses_language_client: bool,
    pub languages: Vec<LanguageEntry>,
    pub grammars: Vec<GrammarEntry>,
    pub themes: Vec<ThemeEntry>,
    pub icon_themes: Vec<IconThemeEntry>,
    pub snippets: Vec<SnippetEntry>,
    /// Languages it starts for, from onLanguage: activation events.
    pub activation_languages: Vec<String>,
    /// What its program would give that can't be had here: how many of each.
    pub program_parts: Vec<(usize, &'static str)>,
}

/// A path inside the extension's folder, from one written relative to `base` (itself inside). Nothing outside the
/// extension's folder is ever read.
fn inside(root: &Path, base: &Path, relative: &str) -> Option<PathBuf> {
    let mut out = base.to_path_buf();
    for part in Path::new(relative.trim()).components() {
        match part {
            std::path::Component::Normal(p) => out.push(p),
            std::path::Component::ParentDir => {
                if out == root || !out.pop() {
                    return None;
                }
            }
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    out.starts_with(root).then_some(out)
}

/// Reads an extension's package.json from its folder, with its %placeholders% filled from package.nls.json.
pub fn read_manifest(root: &Path) -> Result<Manifest, String> {
    let text = std::fs::read_to_string(root.join("package.json")).map_err(|_| crate::i18n::tr("It has no package.json, so it isn't a VS Code extension.").to_string())?;
    let json = crate::vsix_grammar::parse_jsonc(&text).map_err(|e| crate::i18n::tf!("Its package.json can't be read: {e}"))?;
    let nls = std::fs::read_to_string(root.join("package.nls.json")).ok().and_then(|t| crate::vsix_grammar::parse_jsonc(&t).ok());
    Ok(manifest_from(&json, nls.as_ref(), root))
}

fn manifest_from(json: &Value, nls: Option<&Value>, root: &Path) -> Manifest {
    let words = |value: Option<&Value>| -> String {
        let text = value.and_then(Value::as_str).unwrap_or("").trim();
        if let Some(key) = text.strip_prefix('%').and_then(|t| t.strip_suffix('%'))
            && let Some(found) = nls.and_then(|n| n.get(key))
        {
            return found.as_str().or_else(|| found.get("message").and_then(Value::as_str)).unwrap_or("").to_string();
        }
        text.to_string()
    };
    let safe = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')).collect::<String>().to_lowercase();
    let publisher = words(json.get("publisher"));
    let short = words(json.get("name"));
    let id = format!("{}.{}", safe(&publisher), safe(&short));
    let display = words(json.get("displayName"));
    let contributes = json.get("contributes").cloned().unwrap_or(Value::Null);
    let list = |key: &str| contributes.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
    let path_of = |value: Option<&Value>| value.and_then(Value::as_str).and_then(|p| inside(root, root, p));
    let strings = |value: Option<&Value>| value.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
    let languages = list("languages")
        .iter()
        .filter_map(|l| {
            Some(LanguageEntry {
                id: l.get("id")?.as_str()?.to_string(),
                name: l.get("aliases").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str).map(str::to_string),
                extensions: strings(l.get("extensions")),
                filenames: strings(l.get("filenames")),
                first_line: l.get("firstLine").and_then(Value::as_str).map(str::to_string),
                configuration: path_of(l.get("configuration")),
            })
        })
        .collect();
    let grammars = list("grammars")
        .iter()
        .filter_map(|g| Some(GrammarEntry { language: g.get("language").and_then(Value::as_str).map(str::to_string), scope: g.get("scopeName")?.as_str()?.to_string(), path: path_of(g.get("path"))? }))
        .collect();
    let themes = list("themes")
        .iter()
        .filter_map(|t| {
            let label = words(t.get("label"));
            let label = if label.is_empty() { t.get("id").and_then(Value::as_str).unwrap_or("").to_string() } else { label };
            Some(ThemeEntry { label, ui_theme: t.get("uiTheme").and_then(Value::as_str).unwrap_or("").to_string(), path: path_of(t.get("path"))? })
        })
        .collect();
    let icon_themes = list("iconThemes")
        .iter()
        .filter_map(|t| {
            let id = t.get("id")?.as_str()?.to_string();
            let label = words(t.get("label"));
            Some(IconThemeEntry { label: if label.is_empty() { id.clone() } else { label }, id, path: path_of(t.get("path"))? })
        })
        .collect();
    let snippets = list("snippets").iter().filter_map(|s| Some(SnippetEntry { language: s.get("language").and_then(Value::as_str).map(str::to_string), path: path_of(s.get("path"))? })).collect();
    let activation_languages = strings(json.get("activationEvents")).iter().filter_map(|e| e.strip_prefix("onLanguage:")).map(str::to_string).collect();
    let count = |key: &str| match contributes.get(key) {
        Some(Value::Array(a)) => a.len(),
        Some(Value::Object(o)) => o.values().map(|v| v.as_array().map_or(1, Vec::len)).sum(),
        _ => 0,
    };
    let settings = match contributes.get("configuration") {
        Some(Value::Array(sections)) => sections.iter().map(|s| s.get("properties").and_then(Value::as_object).map_or(0, serde_json::Map::len)).sum(),
        Some(section) => section.get("properties").and_then(Value::as_object).map_or(0, serde_json::Map::len),
        None => 0,
    };
    let mut program_parts = Vec::new();
    let parts = [
        (count("commands"), crate::i18n::tr("commands")),
        (settings, crate::i18n::tr("settings")),
        (count("views") + count("viewsContainers"), crate::i18n::tr("panels")),
        (count("debuggers"), crate::i18n::tr("debuggers")),
        (count("customEditors"), crate::i18n::tr("editors of its own")),
        (count("notebooks"), crate::i18n::tr("notebooks")),
        (count("taskDefinitions"), crate::i18n::tr("kinds of task")),
    ];
    for (n, what) in parts {
        if n > 0 {
            program_parts.push((n, what));
        }
    }
    let has_program = json.get("main").is_some() || json.get("browser").is_some();
    let uses_language_client = ["dependencies", "devDependencies"].iter().any(|k| json.get(k).and_then(Value::as_object).is_some_and(|d| d.keys().any(|n| n.starts_with("vscode-languageclient"))));
    Manifest {
        id,
        name: if display.is_empty() { short } else { display },
        publisher,
        version: words(json.get("version")),
        description: words(json.get("description")),
        has_program,
        uses_language_client,
        languages,
        grammars,
        themes,
        icon_themes,
        snippets,
        activation_languages,
        program_parts,
    }
}

// ---------- Language configuration ----------

/// A pair that closes itself: typing the first puts the second after the cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct AutoClose {
    pub open: String,
    pub close: String,
}

/// What a language's configuration says: comment marks, brackets, the pairs that close themselves, and when a
/// line indents or outdents.
#[derive(Debug, Default)]
pub struct LanguageRules {
    pub comments: Option<CommentMarks>,
    pub brackets: Vec<(String, String)>,
    pub auto_close: Vec<AutoClose>,
    pub surround: Vec<(String, String)>,
    /// The characters a pair closes itself before; anywhere else (before a word, say), it doesn't.
    pub close_before: String,
    pub increase: Option<syntect::parsing::Regex>,
    pub decrease: Option<syntect::parsing::Regex>,
    pub indent_next: Option<syntect::parsing::Regex>,
}

/// Leaks a string to give it for good: comment marks are kept as the built-in ones are. Each language's are
/// leaked once each time the extensions are read, which happens only when they change.
fn forever(text: &str) -> &'static str {
    Box::leak(text.to_string().into_boxed_str())
}

/// A regular expression from a language configuration: a string, or a pattern with flags.
fn config_regex(value: Option<&Value>) -> Option<syntect::parsing::Regex> {
    let (pattern, flags) = match value? {
        Value::String(s) => (s.clone(), String::new()),
        Value::Object(o) => (o.get("pattern")?.as_str()?.to_string(), o.get("flags").and_then(Value::as_str).unwrap_or("").to_string()),
        _ => return None,
    };
    let pattern = if flags.contains('i') { format!("(?i){pattern}") } else { pattern };
    syntect::parsing::Regex::try_compile(&pattern).is_none().then(|| syntect::parsing::Regex::new(pattern))
}

pub fn read_rules(json: &Value) -> LanguageRules {
    let pair = |v: &Value| -> Option<(String, String)> {
        match v {
            Value::Array(a) => Some((a.first()?.as_str()?.to_string(), a.get(1)?.as_str()?.to_string())),
            Value::Object(o) => Some((o.get("open")?.as_str()?.to_string(), o.get("close")?.as_str()?.to_string())),
            _ => None,
        }
    };
    let pairs = |key: &str| json.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(pair).filter(|(o, c)| !o.is_empty() && !c.is_empty()).collect::<Vec<_>>()).unwrap_or_default();
    let comments = json.get("comments");
    let line = comments.and_then(|c| c.get("lineComment")).and_then(|l| l.as_str().or_else(|| l.get("comment").and_then(Value::as_str))).filter(|s| !s.is_empty());
    let block = comments.and_then(|c| c.get("blockComment")).and_then(pair);
    let marks = (line.is_some() || block.is_some()).then(|| CommentMarks { line: line.map(forever), block: block.as_ref().map(|(o, c)| (forever(o), forever(c))) });
    let indentation = json.get("indentationRules");
    let brackets = pairs("brackets");
    let mut auto_close: Vec<AutoClose> = pairs("autoClosingPairs").into_iter().map(|(open, close)| AutoClose { open, close }).collect();
    // Longer openers first, so /** is found before /*.
    auto_close.sort_by_key(|a| std::cmp::Reverse(a.open.chars().count()));
    LanguageRules {
        comments: marks,
        surround: pairs("surroundingPairs"),
        brackets,
        auto_close,
        close_before: json.get("autoCloseBefore").and_then(Value::as_str).unwrap_or(";:.,=}])> \n\t").to_string(),
        increase: config_regex(indentation.and_then(|i| i.get("increaseIndentPattern"))),
        decrease: config_regex(indentation.and_then(|i| i.get("decreaseIndentPattern"))),
        indent_next: config_regex(indentation.and_then(|i| i.get("indentNextLinePattern"))),
    }
}

impl LanguageRules {
    /// The closer to put after the cursor, once `before` (the line up to the cursor, with what was just typed)
    /// ends with an opener and `after` (the rest of the line) allows it.
    pub fn closing_for(&self, before: &str, after: &str) -> Option<&str> {
        let next = after.chars().next();
        if next.is_some_and(|c| !self.close_before.contains(c)) {
            return None;
        }
        let pair = self.auto_close.iter().find(|p| before.ends_with(&p.open))?;
        if pair.open == pair.close {
            // A quote straight after a letter is an apostrophe, or the end of a word in quotes.
            let ahead = before[..before.len() - pair.open.len()].chars().last();
            if ahead.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                return None;
            }
        }
        Some(pair.close.as_str())
    }

    /// The closer for an opener typed with text selected, to put round it.
    pub fn surrounding(&self, typed: &str) -> Option<&str> {
        self.surround.iter().find(|(open, _)| open == typed).map(|(_, close)| close.as_str())
    }

    pub fn is_closer(&self, typed: &str) -> bool {
        self.auto_close.iter().any(|p| p.close == typed)
    }

    /// Whether the line after this one goes in a step, as this one opens something.
    pub fn indents_after(&self, line: &str) -> bool {
        self.increase.as_ref().is_some_and(|r| r.is_match(line)) || self.indent_next.as_ref().is_some_and(|r| r.is_match(line))
    }

    /// Whether this line comes out a step, as it closes something.
    pub fn outdents(&self, line: &str) -> bool {
        self.decrease.as_ref().is_some_and(|r| r.is_match(line))
    }
}

// ---------- Snippets ----------

/// A snippet an extension gives, in VS Code's own way of writing them.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtSnippet {
    pub name: String,
    pub prefix: String,
    pub description: String,
    pub body: String,
    /// VS Code's ids for the languages it's for; none for every language.
    pub languages: Vec<String>,
    pub extension: String,
}

/// Reads a snippets file, for the language its entry in package.json gives (or that each snippet gives itself).
pub fn read_snippets(json: &Value, language: Option<&str>, extension: &str) -> Vec<ExtSnippet> {
    let Some(map) = json.as_object() else { return Vec::new() };
    let mut out = Vec::new();
    for (name, snippet) in map {
        let Some(body) = (match snippet.get("body") {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Array(lines)) => Some(lines.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n")),
            _ => None,
        }) else {
            continue;
        };
        let prefix = match snippet.get("prefix") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(list)) => list.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "),
            _ => String::new(),
        };
        let languages = match (language, snippet.get("scope").and_then(Value::as_str)) {
            (Some(language), _) => vec![language.to_string()],
            (None, Some(scope)) => scope.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            (None, None) => Vec::new(),
        };
        out.push(ExtSnippet { name: name.clone(), prefix, description: snippet.get("description").and_then(Value::as_str).unwrap_or("").to_string(), body, languages, extension: extension.to_string() });
    }
    out
}

/// What fills a snippet's variables.
pub struct Fill<'a> {
    pub date: chrono::DateTime<chrono::Local>,
    pub file: &'a str,
    pub selection: &'a str,
}

/// A VS Code snippet's body with its tab stops and variables filled: the text before where the cursor goes, and
/// after it. Placeholders become their text, a choice its first option; the cursor goes to $0, or else the first
/// tab stop, or else the end.
pub fn expand_snippet(body: &str, fill: &Fill) -> (String, String) {
    let mut out = String::new();
    let mut stops: Vec<(u32, usize)> = Vec::new();
    expand_into(&body.chars().collect::<Vec<_>>(), &mut 0, &mut out, &mut stops, fill, false);
    let cursor = stops.iter().find(|(n, _)| *n == 0).or_else(|| stops.iter().filter(|(n, _)| *n > 0).min_by_key(|(n, _)| *n)).map_or(out.len(), |(_, at)| *at);
    (out[..cursor].to_string(), out[cursor..].to_string())
}

fn variable(name: &str, fill: &Fill) -> Option<String> {
    let d = &fill.date;
    let base = Path::new(fill.file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Some(match name {
        "TM_SELECTED_TEXT" | "SELECTION" => fill.selection.to_string(),
        "TM_FILENAME" => fill.file.to_string(),
        "TM_FILENAME_BASE" => base,
        "CURRENT_YEAR" => d.format("%Y").to_string(),
        "CURRENT_YEAR_SHORT" => d.format("%y").to_string(),
        "CURRENT_MONTH" => d.format("%m").to_string(),
        "CURRENT_MONTH_NAME" => d.format("%B").to_string(),
        "CURRENT_MONTH_NAME_SHORT" => d.format("%b").to_string(),
        "CURRENT_DATE" => d.format("%d").to_string(),
        "CURRENT_DAY_NAME" => d.format("%A").to_string(),
        "CURRENT_DAY_NAME_SHORT" => d.format("%a").to_string(),
        "CURRENT_HOUR" => d.format("%H").to_string(),
        "CURRENT_MINUTE" => d.format("%M").to_string(),
        "CURRENT_SECOND" => d.format("%S").to_string(),
        "CURRENT_SECONDS_UNIX" => d.timestamp().to_string(),
        "UUID" => String::new(),
        "BLOCK_COMMENT_START" | "BLOCK_COMMENT_END" | "LINE_COMMENT" | "TM_CURRENT_LINE" | "TM_CURRENT_WORD" | "TM_LINE_INDEX" | "TM_LINE_NUMBER" | "TM_DIRECTORY" | "TM_FILEPATH" | "RELATIVE_FILEPATH" | "CLIPBOARD" | "WORKSPACE_NAME" | "WORKSPACE_FOLDER" | "CURSOR_INDEX" | "CURSOR_NUMBER" => String::new(),
        _ => return None,
    })
}

/// Expands from `at` until the end, or (when `nested`) the } that closes a placeholder.
fn expand_into(chars: &[char], at: &mut usize, out: &mut String, stops: &mut Vec<(u32, usize)>, fill: &Fill, nested: bool) {
    while *at < chars.len() {
        let c = chars[*at];
        match c {
            '\\' if *at + 1 < chars.len() && matches!(chars[*at + 1], '$' | '}' | '\\' | ',' | '|') => {
                out.push(chars[*at + 1]);
                *at += 2;
            }
            '}' if nested => {
                *at += 1;
                return;
            }
            '$' => {
                *at += 1;
                let digits: String = chars[*at..].iter().take_while(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() {
                    *at += digits.len();
                    stops.push((digits.parse().unwrap_or(0), out.len()));
                    continue;
                }
                let name: String = chars[*at..].iter().take_while(|c| c.is_ascii_alphanumeric() || **c == '_').collect();
                if !name.is_empty() && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') {
                    *at += name.len();
                    out.push_str(&variable(&name, fill).unwrap_or_else(|| name.clone()));
                    continue;
                }
                if chars.get(*at) != Some(&'{') {
                    out.push('$');
                    continue;
                }
                *at += 1;
                let inner: String = chars[*at..].iter().take_while(|c| c.is_ascii_alphanumeric() || **c == '_').collect();
                *at += inner.len();
                let number = inner.parse::<u32>().ok();
                if let Some(n) = number {
                    stops.push((n, out.len()));
                }
                match chars.get(*at) {
                    Some('}') => {
                        *at += 1;
                        if number.is_none() {
                            out.push_str(&variable(&inner, fill).unwrap_or_default());
                        }
                    }
                    Some(':') => {
                        *at += 1;
                        // A variable's value if it has one, else its default; a tab stop's placeholder.
                        let value = if number.is_none() { variable(&inner, fill).filter(|v| !v.is_empty()) } else { None };
                        match value {
                            Some(value) => {
                                let mut ignored = String::new();
                                expand_into(chars, at, &mut ignored, &mut Vec::new(), fill, true);
                                out.push_str(&value);
                            }
                            None => expand_into(chars, at, out, stops, fill, true),
                        }
                    }
                    Some('|') => {
                        // A choice: its first option.
                        *at += 1;
                        let mut first = String::new();
                        let mut taking = true;
                        while *at < chars.len() && !(chars[*at] == '|' && chars.get(*at + 1) == Some(&'}')) {
                            match chars[*at] {
                                '\\' if *at + 1 < chars.len() => {
                                    if taking {
                                        first.push(chars[*at + 1]);
                                    }
                                    *at += 1;
                                }
                                ',' => taking = false,
                                other if taking => first.push(other),
                                _ => {}
                            }
                            *at += 1;
                        }
                        *at = (*at + 2).min(chars.len());
                        out.push_str(&first);
                    }
                    Some('/') => {
                        // A transform, which changes the value by a regular expression: the value as it is.
                        let mut slashes = 0;
                        while *at < chars.len() && !(chars[*at] == '}' && slashes >= 3) {
                            if chars[*at] == '/' && chars.get(at.wrapping_sub(1)) != Some(&'\\') {
                                slashes += 1;
                            }
                            *at += 1;
                        }
                        *at = (*at + 1).min(chars.len());
                        if number.is_none() {
                            out.push_str(&variable(&inner, fill).unwrap_or_default());
                        }
                    }
                    _ => {}
                }
            }
            _ => {
                out.push(c);
                *at += 1;
            }
        }
    }
}

// ---------- File icon themes ----------

/// A file icon theme: a picture for each file, by its name, its ending, or its language.
#[derive(Debug, Default)]
pub struct IconTheme {
    /// extension id/theme id.
    pub key: String,
    pub label: String,
    definitions: HashMap<String, PathBuf>,
    file: Option<String>,
    endings: HashMap<String, String>,
    names: HashMap<String, String>,
    languages: HashMap<String, String>,
    light: Option<Box<IconTheme>>,
}

pub fn read_icon_theme(json: &Value, root: &Path, json_folder: &Path, key: &str, label: &str) -> IconTheme {
    let mut definitions = HashMap::new();
    if let Some(map) = json.get("iconDefinitions").and_then(Value::as_object) {
        for (id, definition) in map {
            // Pictures only: an icon drawn from a font's character (as Seti's are) is left to Limoniter's badges.
            if let Some(path) = definition.get("iconPath").and_then(Value::as_str).and_then(|p| inside(root, json_folder, p)) {
                definitions.insert(id.clone(), path);
            }
        }
    }
    let mut theme = icon_mappings(json);
    theme.key = key.to_string();
    theme.label = label.to_string();
    theme.definitions = definitions;
    theme
}

fn icon_mappings(json: &Value) -> IconTheme {
    let map = |key: &str| json.get(key).and_then(Value::as_object).map(|m| m.iter().filter_map(|(k, v)| Some((k.to_lowercase(), v.as_str()?.to_string()))).collect()).unwrap_or_default();
    IconTheme {
        file: json.get("file").and_then(Value::as_str).map(str::to_string),
        endings: map("fileExtensions"),
        names: map("fileNames"),
        languages: map("languageIds"),
        light: json.get("light").filter(|l| l.is_object()).map(|l| Box::new(icon_mappings(l))),
        ..IconTheme::default()
    }
}

impl IconTheme {
    /// The picture for a file, by its name: its whole name first, then its endings, longest first ("test.ts"
    /// before "ts"), then its language, then the theme's picture for any file.
    pub fn icon_for(&self, name: &str, language: Option<&str>, light: bool) -> Option<&Path> {
        let lower = name.to_lowercase();
        let variant = self.light.as_deref().filter(|_| light);
        let look = |pick: fn(&IconTheme) -> &HashMap<String, String>, key: &str| variant.and_then(|v| pick(v).get(key)).or_else(|| pick(self).get(key));
        let id = look(|t| &t.names, &lower)
            .or_else(|| lower.match_indices('.').find_map(|(at, _)| look(|t| &t.endings, &lower[at + 1..])))
            .or_else(|| language.and_then(|l| look(|t| &t.languages, l)))
            .or_else(|| variant.and_then(|v| v.file.as_ref()).or(self.file.as_ref()))?;
        self.definitions.get(id).map(PathBuf::as_path)
    }
}

// ---------- Language servers ----------

/// A language server an extension starts, as it can be started here.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtServer {
    pub extension: String,
    /// VS Code's ids for the languages it looks after.
    pub languages: Vec<String>,
    pub command: String,
    /// The program that must be there for it to run: Node.js for a server written in JavaScript, the server
    /// itself otherwise.
    pub program: String,
}

/// Servers some well-known extensions start, which they find or download for themselves rather than carry.
const KNOWN_SERVERS: &[(&str, &str, &[&str])] = &[
    ("ziglang.vscode-zig", "zls", &["zig"]),
    ("tamasfe.even-better-toml", "taplo lsp stdio", &["toml"]),
    ("golang.go", "gopls", &["go"]),
    ("rust-lang.rust-analyzer", "rust-analyzer", &["rust"]),
    ("llvm-vs-code-extensions.vscode-clangd", "clangd", &["c", "cpp", "objective-c", "objective-cpp"]),
    ("sumneko.lua", "lua-language-server", &["lua"]),
    ("haskell.haskell", "haskell-language-server-wrapper --lsp", &["haskell"]),
    ("denoland.vscode-deno", "deno lsp", &["javascript", "typescript"]),
    ("ocamllabs.ocaml-platform", "ocamllsp", &["ocaml"]),
    ("scalameta.metals", "metals", &["scala"]),
    ("dart-code.dart-code", "dart language-server --protocol=lsp", &["dart"]),
    ("nimlang.nimlang", "nimlangserver", &["nim"]),
    ("myriad-dreamin.tinymist", "tinymist", &["typst"]),
    ("mesonbuild.mesonbuild", "mesonlsp --lsp", &["meson"]),
    ("vadimcn.vscode-lldb", "", &[]),
];

/// Whether a file is a program for this computer, by its first bytes.
fn is_native_program(head: &[u8]) -> bool {
    if cfg!(windows) {
        head.starts_with(b"MZ")
    } else if cfg!(target_os = "macos") {
        matches!(head.get(..4), Some([0xcf, 0xfa, 0xed, 0xfe] | [0xca, 0xfe, 0xba, 0xbe] | [0xce, 0xfa, 0xed, 0xfe]))
    } else {
        head.starts_with(b"\x7fELF")
    }
}

fn looks_like_server(stem: &str) -> bool {
    let stem = stem.to_lowercase();
    stem.ends_with("ls") || stem.contains("lsp") || stem.contains("server") || stem.contains("analyzer") || stem.contains("language")
}

/// The files of an extension, outside its node_modules, a few folders down at most.
fn files_of(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut level = vec![root.to_path_buf()];
    for _ in 0..6 {
        let mut next = Vec::new();
        for folder in &level {
            let Ok(entries) = std::fs::read_dir(folder) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if path.is_dir() {
                    if name != "node_modules" && !name.starts_with('.') {
                        next.push(path);
                    }
                } else {
                    out.push(path);
                }
            }
        }
        level = next;
    }
    out.sort();
    out
}

fn quoted(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

/// The language server an extension starts, if one can be found: a program it carries, a Node.js server in it,
/// or the one it's known to use.
pub fn find_server(root: &Path, manifest: &Manifest) -> Option<ExtServer> {
    let mut languages: Vec<String> = manifest.languages.iter().map(|l| l.id.clone()).collect();
    for language in &manifest.activation_languages {
        if !languages.contains(language) {
            languages.push(language.clone());
        }
    }
    let known = KNOWN_SERVERS.iter().find(|(id, _, _)| *id == manifest.id);
    if let Some((_, _, known_languages)) = known {
        for language in *known_languages {
            if !languages.iter().any(|l| l == language) {
                languages.push(language.to_string());
            }
        }
    }
    if languages.is_empty() || !manifest.has_program {
        return None;
    }
    let files = files_of(root);
    let program_text = |path: &Path| std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    // A program it carries, built for this computer.
    for path in &files {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if !looks_like_server(&stem) {
            continue;
        }
        let mut head = [0u8; 4];
        let read = std::fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
        if read == 4 && is_native_program(&head) {
            return Some(ExtServer { extension: manifest.id.clone(), languages, command: quoted(path), program: path.display().to_string() });
        }
    }
    // A server written in JavaScript, which its program starts with Node.js, and names.
    if manifest.uses_language_client || files.iter().any(|p| p.extension().is_some_and(|e| e == "js") && program_text(p).contains("LanguageClient")) {
        let mains: Vec<String> = files.iter().filter(|p| p.extension().is_some_and(|e| e == "js" || e == "cjs") && !looks_like_server(&p.file_stem().unwrap_or_default().to_string_lossy())).map(|p| program_text(p)).collect();
        for path in &files {
            let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let is_script = path.extension().is_some_and(|e| e == "js" || e == "cjs");
            if is_script && looks_like_server(&name) && mains.iter().any(|m| m.contains(&name)) {
                return Some(ExtServer { extension: manifest.id.clone(), languages, command: format!("node {} --stdio", quoted(path)), program: "node".into() });
            }
        }
    }
    let (_, command, _) = known?;
    let program = crate::emulators::split_options(command).into_iter().next()?;
    Some(ExtServer { extension: manifest.id.clone(), languages, command: command.to_string(), program })
}

// ---------- VS Code's names for languages ----------

/// VS Code's ids for languages Limoniter colours, and the names Limoniter has for them.
const LANGUAGE_IDS: &[(&str, &str)] = &[
    ("c", "C"),
    ("cpp", "C++"),
    ("csharp", "C#"),
    ("css", "CSS"),
    ("scss", "SCSS"),
    ("less", "LESS"),
    ("go", "Go"),
    ("html", "HTML"),
    ("java", "Java"),
    ("javascript", "JavaScript"),
    ("javascriptreact", "JavaScript (Babel)"),
    ("typescript", "TypeScript"),
    ("typescriptreact", "TypeScriptReact"),
    ("json", "JSON"),
    ("jsonc", "JSON"),
    ("lua", "Lua"),
    ("markdown", "Markdown"),
    ("python", "Python"),
    ("rust", "Rust"),
    ("shellscript", "Bourne Again Shell (bash)"),
    ("xml", "XML"),
    ("yaml", "YAML"),
    ("sql", "SQL"),
    ("php", "PHP"),
    ("ruby", "Ruby"),
    ("perl", "Perl"),
    ("bat", "Batch File"),
    ("powershell", "PowerShell"),
    ("makefile", "Makefile"),
    ("dockerfile", "Dockerfile"),
    ("toml", "TOML"),
    ("zig", "Zig"),
    ("haskell", "Haskell"),
    ("swift", "Swift"),
    ("kotlin", "Kotlin"),
    ("scala", "Scala"),
    ("dart", "Dart"),
    ("elixir", "Elixir"),
    ("erlang", "Erlang"),
    ("clojure", "Clojure"),
    ("latex", "LaTeX"),
    ("tex", "TeX"),
    ("diff", "Diff"),
    ("r", "R"),
    ("groovy", "Groovy"),
    ("objective-c", "Objective-C"),
    ("objective-cpp", "Objective-C++"),
    ("vb", "Visual Basic"),
    ("fsharp", "F#"),
    ("ocaml", "OCaml"),
    ("julia", "Julia"),
    ("nim", "Nim"),
    ("d", "D"),
    ("fortran", "Fortran (Modern)"),
    ("pascal", "Pascal"),
    ("asm", "Assembly x86_64"),
    ("ini", "INI"),
    ("cmake", "CMake"),
    ("graphql", "GraphQL"),
    ("vue", "Vue Component"),
    ("svelte", "Svelte"),
];

// ---------- Installed extensions, and what they gave ----------

/// One installed extension: where it is, what it says, whether it's on, and what it gave or couldn't.
#[derive(Clone, Debug)]
pub struct Installed {
    pub folder: PathBuf,
    pub manifest: Manifest,
    pub on: bool,
    /// What it gives, a line each.
    pub gave: Vec<String>,
    /// What couldn't be read or used, a line each.
    pub problems: Vec<String>,
}

/// A language an extension gives, with the grammar that colours it if it has one.
#[derive(Debug)]
pub struct LoadedLanguage {
    pub id: String,
    pub name: String,
    pub scope: Option<String>,
    pub rules: Option<Arc<LanguageRules>>,
}

/// A grammar ready for the highlighter: Sublime's YAML, and what it's for.
#[derive(Debug, Clone)]
pub struct LoadedGrammar {
    pub extension: String,
    pub extension_name: String,
    pub name: String,
    pub scope: String,
    pub yaml: String,
}

pub struct LoadedTheme {
    pub document: crate::theme::DocumentTheme,
    pub window: crate::theme::WindowTheme,
    pub scheme: Arc<syntect::highlighting::Theme>,
}

/// Everything the extensions that are on give, read once each time they change.
#[derive(Default)]
pub struct Loaded {
    pub extensions: Vec<Installed>,
    pub off: BTreeSet<String>,
    /// The file icon theme chosen, as extension id/theme id.
    pub icon_choice: Option<String>,
    /// Every icon theme there is to choose: its key, its label.
    pub icon_themes: Vec<(String, String)>,
    pub icons: Option<IconTheme>,
    pub themes: Vec<LoadedTheme>,
    pub grammars: Vec<LoadedGrammar>,
    pub languages: Vec<LoadedLanguage>,
    pub snippets: Vec<ExtSnippet>,
    pub servers: Vec<ExtServer>,
}

fn read_json_file(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| crate::i18n::tf!("{} couldn't be read: {e}", file_name(path)))?;
    crate::vsix_grammar::read_structured(path, &text).map_err(|e| crate::i18n::tf!("{}: {e}", file_name(path)))
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// Endings as Limoniter shows them: .zig, .zon.
fn endings_list(language: &LanguageEntry) -> String {
    language.extensions.iter().map(|e| if e.starts_with('.') { e.clone() } else { format!(".{e}") }).chain(language.filenames.iter().cloned()).collect::<Vec<_>>().join(", ")
}

/// Reads every extension in the folder, and takes in what those that are on give.
pub fn load_from(folder: &Path, off: &BTreeSet<String>, icon_choice: Option<&str>) -> Loaded {
    let mut loaded = Loaded { off: off.clone(), icon_choice: icon_choice.map(str::to_string), ..Loaded::default() };
    let Ok(entries) = std::fs::read_dir(folder) else { return loaded };
    let mut folders: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir() && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).collect();
    folders.sort();
    for root in folders {
        let manifest = match read_manifest(&root) {
            Ok(manifest) => manifest,
            Err(problem) => {
                let id = file_name(&root);
                loaded.extensions.push(Installed { folder: root, manifest: Manifest { name: id.clone(), id, ..Manifest::default() }, on: false, gave: Vec::new(), problems: vec![problem] });
                continue;
            }
        };
        let on = !off.contains(&manifest.id);
        let mut installed = Installed { folder: root.clone(), manifest, on, gave: Vec::new(), problems: Vec::new() };
        take_in(&mut loaded, &mut installed, on);
        loaded.extensions.push(installed);
    }
    loaded.extensions.sort_by_key(|e| e.manifest.name.to_lowercase());
    loaded
}

/// Reads what one extension gives, noting each part, and keeps it if the extension is on.
fn take_in(loaded: &mut Loaded, installed: &mut Installed, on: bool) {
    let manifest = installed.manifest.clone();
    let root = installed.folder.clone();
    let id = manifest.id.clone();
    // A language's name as the extension gives it, for saying what it gave, else as Limoniter knows it.
    let language_name = |language: &str, known: &[LoadedLanguage]| manifest.languages.iter().find(|l| l.id == language).and_then(|l| l.name.clone()).unwrap_or_else(|| name_for_id_in(language, known));
    // Colour themes.
    let mut theme_names = Vec::new();
    for entry in &manifest.themes {
        match crate::vsix_theme::read(&entry.path) {
            Ok(file) => {
                let converted = crate::vsix_theme::convert(&file, &entry.label, &scheme_name(&entry.label), &entry.ui_theme);
                theme_names.push(entry.label.clone());
                if on {
                    loaded.themes.push(LoadedTheme { document: converted.document, window: converted.window, scheme: Arc::new(converted.scheme) });
                }
            }
            Err(problem) => installed.problems.push(crate::i18n::tf!("The theme {}: {problem}", entry.label)),
        }
    }
    if !theme_names.is_empty() {
        installed.gave.push(crate::i18n::tf!("Colour themes, as document and window themes: {}", theme_names.join(", ")));
    }
    // Languages: their grammars and configuration.
    for language in &manifest.languages {
        let name = language.name.clone().unwrap_or_else(|| language.id.clone());
        let mut scope = None;
        if let Some(grammar) = manifest.grammars.iter().find(|g| g.language.as_deref() == Some(language.id.as_str())) {
            let mut endings: Vec<String> = language.extensions.iter().map(|e| e.trim_start_matches('.').to_string()).collect();
            endings.extend(language.filenames.iter().cloned());
            let converted = read_json_file(&grammar.path).and_then(|json| crate::vsix_grammar::convert(&json, &crate::vsix_grammar::GrammarInfo { name: &name, endings: &endings, first_line: language.first_line.as_deref() }));
            match converted {
                Ok(converted) => {
                    let mut line = crate::i18n::tf!("Colouring for {name}");
                    let endings = endings_list(language);
                    if !endings.is_empty() {
                        line += &format!(" ({endings})");
                    }
                    if converted.left_out > 0 {
                        line += &crate::i18n::tf!(", with {} of its patterns left out, as Limoniter can't use them", converted.left_out);
                    }
                    installed.gave.push(line);
                    scope = Some(converted.scope.clone());
                    if on {
                        loaded.grammars.push(LoadedGrammar { extension: id.clone(), extension_name: manifest.name.clone(), name: name.clone(), scope: converted.scope, yaml: converted.yaml });
                    }
                }
                Err(problem) => installed.problems.push(crate::i18n::tf!("The grammar for {name}: {problem}")),
            }
        }
        let rules = language.configuration.as_ref().and_then(|path| match read_json_file(path) {
            Ok(json) => Some(Arc::new(read_rules(&json))),
            Err(problem) => {
                installed.problems.push(crate::i18n::tf!("The configuration for {name}: {problem}"));
                None
            }
        });
        if rules.is_some() {
            installed.gave.push(crate::i18n::tf!("Comment marks, brackets that close themselves and indenting for {name}"));
        }
        if on && (scope.is_some() || rules.is_some()) {
            loaded.languages.push(LoadedLanguage { id: language.id.clone(), name, scope, rules });
        }
    }
    let injections = manifest.grammars.iter().filter(|g| g.language.is_none()).count();
    if injections > 0 {
        installed.problems.push(crate::i18n::tf!("{injections} grammars that only add to other languages' colouring are left out."));
    }
    // Snippets.
    let mut snippets = Vec::new();
    for entry in &manifest.snippets {
        match read_json_file(&entry.path) {
            Ok(json) => snippets.extend(read_snippets(&json, entry.language.as_deref(), &id)),
            Err(problem) => installed.problems.push(crate::i18n::tf!("Snippets: {problem}")),
        }
    }
    if !snippets.is_empty() {
        let mut languages: Vec<String> = snippets.iter().flat_map(|s| s.languages.iter().map(|l| language_name(l, &loaded.languages))).collect::<BTreeSet<_>>().into_iter().collect();
        if languages.is_empty() {
            languages.push(crate::i18n::tr("every language").to_string());
        }
        installed.gave.push(crate::i18n::tf!("{} snippets, in Edit \u{203A} Snippets, for {}", snippets.len(), languages.join(", ")));
        if on {
            loaded.snippets.extend(snippets);
        }
    }
    // File icon themes.
    for entry in &manifest.icon_themes {
        let key = format!("{id}/{}", entry.id);
        if !entry.path.is_file() {
            installed.problems.push(crate::i18n::tf!("The file icons {}: its file isn't there.", entry.label));
            continue;
        }
        installed.gave.push(crate::i18n::tf!("File icons: {}", entry.label));
        if on {
            loaded.icon_themes.push((key.clone(), entry.label.clone()));
            if loaded.icon_choice.as_deref() == Some(key.as_str()) {
                match read_json_file(&entry.path) {
                    Ok(json) => loaded.icons = Some(read_icon_theme(&json, &root, entry.path.parent().unwrap_or(root.as_path()), &key, &entry.label)),
                    Err(problem) => installed.problems.push(crate::i18n::tf!("The file icons {}: {problem}", entry.label)),
                }
            }
        }
    }
    // A language server.
    if let Some(server) = find_server(&root, &manifest) {
        let languages = server.languages.iter().map(|l| language_name(l, &loaded.languages)).collect::<Vec<_>>().join(", ");
        let line = if server.program == "node" {
            crate::i18n::tf!("A language server for {languages}, run with Node.js")
        } else if Path::new(&server.program).is_absolute() {
            crate::i18n::tf!("A language server for {languages}, which it carries")
        } else {
            crate::i18n::tf!("A language server for {languages}: {}", server.program)
        };
        installed.gave.push(line);
        if on {
            loaded.servers.push(server);
        }
    }
    // What its program would have given.
    if manifest.has_program && !manifest.program_parts.is_empty() {
        let parts: Vec<String> = manifest.program_parts.iter().map(|(n, what)| format!("{n} {what}")).collect();
        installed.problems.push(crate::i18n::tf!("Its own program (JavaScript) isn't run, so these aren't here: {}.", parts.join(", ")));
    }
}

/// The name an extension theme's syntax colours go by, kept apart from the built-in schemes' names.
fn scheme_name(label: &str) -> String {
    if crate::highlight::SCHEMES.iter().any(|s| s.as_name() == label) || label == crate::highlight::NO_SYNTAX_COLOURS { format!("{label} (VS Code)") } else { label.to_string() }
}

/// Whether an extension gives anything Limoniter can use: if it doesn't, it isn't installed.
fn usable(installed: &Installed) -> bool {
    // Everything given is on its gave list; a theme or grammar that couldn't be read isn't.
    !installed.gave.is_empty()
}

/// Why an extension can't be installed, in plain words.
fn refusal(installed: &Installed) -> String {
    let m = &installed.manifest;
    let mut text = if m.has_program {
        crate::i18n::tf!("{} works through its own program (JavaScript), which Limoniter doesn't run, and it gives nothing Limoniter can take in without it: no colour themes, grammars, snippets, file icons or language settings. So it wasn't installed.", m.name)
    } else {
        crate::i18n::tf!("{} gives nothing Limoniter can take in: no colour themes, grammars, snippets, file icons or language settings. So it wasn't installed.", m.name)
    };
    if !installed.problems.is_empty() {
        text += "\n\n";
        text += &installed.problems.join("\n");
    }
    text
}

// ---------- Installing and removing ----------

/// Unpacks a .vsix into the extensions folder, replacing the same extension if it's there, after making sure it
/// gives something that can be used. Gives the extension as installed.
pub fn install(vsix: Vec<u8>, folder: &Path) -> Result<Installed, String> {
    let volume = crate::media::open(Arc::new(vsix), "extension.zip").map_err(|_| crate::i18n::tr("It isn't a .vsix: a .vsix is a zip, and this can't be read as one.").to_string())?;
    let top = volume.list(None)?;
    let extension = top.iter().find(|e| e.is_dir && e.name == "extension").ok_or(crate::i18n::tr("It isn't a VS Code extension: there's no extension folder in it."))?;
    std::fs::create_dir_all(folder).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", folder.display()))?;
    let incoming = folder.join(format!(".incoming-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&incoming);
    let mut total = 0;
    let unpacked = unpack(&*volume, extension, &incoming, &mut total).and_then(|()| read_manifest(&incoming));
    let manifest = match unpacked {
        Ok(manifest) => manifest,
        Err(problem) => {
            let _ = std::fs::remove_dir_all(&incoming);
            return Err(problem);
        }
    };
    if manifest.id.starts_with('.') || manifest.id.ends_with('.') {
        let _ = std::fs::remove_dir_all(&incoming);
        return Err(crate::i18n::tr("Its package.json doesn't give its publisher and name.").into());
    }
    let mut installed = Installed { folder: incoming.clone(), manifest: manifest.clone(), on: true, gave: Vec::new(), problems: Vec::new() };
    take_in(&mut Loaded::default(), &mut installed, false);
    if !usable(&installed) {
        let _ = std::fs::remove_dir_all(&incoming);
        return Err(refusal(&installed));
    }
    let target = folder.join(&manifest.id);
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&incoming, &target).map_err(|e| {
        let _ = std::fs::remove_dir_all(&incoming);
        crate::i18n::tf!("{} couldn't be put in place: {e}", target.display())
    })?;
    let mut installed = Installed { folder: target.clone(), manifest: read_manifest(&target)?, on: true, gave: Vec::new(), problems: Vec::new() };
    take_in(&mut Loaded::default(), &mut installed, false);
    Ok(installed)
}

fn unpack(volume: &dyn crate::media::Volume, dir: &crate::media::Entry, target: &Path, total: &mut u64) -> Result<(), String> {
    std::fs::create_dir_all(target).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", target.display()))?;
    for entry in volume.list(Some(dir))? {
        // Nothing in the zip is let out of the folder.
        if entry.name.contains(['/', '\\', ':']) || entry.name == ".." || entry.name == "." || entry.name.is_empty() {
            continue;
        }
        let path = target.join(&entry.name);
        if entry.is_dir {
            unpack(volume, &entry, &path, total)?;
            continue;
        }
        *total += entry.size;
        if *total > MOST_UNPACKED {
            return Err(crate::i18n::tr("It unpacks to more than a gigabyte, which is more than an extension should.").into());
        }
        let bytes = volume.read(&entry)?;
        std::fs::write(&path, &bytes).map_err(|e| crate::i18n::tf!("{} couldn't be written: {e}", path.display()))?;
        // A program it carries (a language server) is let run, as unzipping it would on a Mac or Linux.
        #[cfg(unix)]
        if bytes.starts_with(b"\x7fELF") || bytes.starts_with(&[0xcf, 0xfa, 0xed, 0xfe]) || bytes.starts_with(&[0xca, 0xfe, 0xba, 0xbe]) || bytes.starts_with(b"#!") {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
    }
    Ok(())
}

/// Removes an installed extension's folder.
pub fn remove(installed: &Installed) -> Result<(), String> {
    std::fs::remove_dir_all(&installed.folder).map_err(|e| crate::i18n::tf!("{} couldn't be removed: {e}", installed.folder.display()))
}

// ---------- Open VSX ----------

/// An extension found on Open VSX.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// namespace.name
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub downloads: u64,
}

fn encode(text: &str) -> String {
    text.bytes().map(|b| if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn get(address: &str, most: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let response = ureq::get(address).header("User-Agent", "Limoniter").call().map_err(|e| crate::i18n::tf!("Open VSX couldn't be reached: {e}"))?;
    let mut bytes = Vec::new();
    response.into_body().into_reader().take(most).read_to_end(&mut bytes).map_err(|e| crate::i18n::tf!("The download stopped: {e}"))?;
    Ok(bytes)
}

/// Searches Open VSX.
pub fn search(query: &str) -> Result<Vec<Found>, String> {
    let bytes = get(&format!("{OPEN_VSX}/api/-/search?query={}&size=30", encode(query.trim())), 16 * 1024 * 1024)?;
    parse_search(&String::from_utf8_lossy(&bytes))
}

pub fn parse_search(text: &str) -> Result<Vec<Found>, String> {
    let json: Value = serde_json::from_str(text).map_err(|_| crate::i18n::tr("Open VSX's answer wasn't what was expected.").to_string())?;
    let list = json.get("extensions").and_then(Value::as_array).ok_or(crate::i18n::tr("Open VSX's answer wasn't what was expected."))?;
    Ok(list
        .iter()
        .filter_map(|e| {
            let text = |key: &str| e.get(key).and_then(Value::as_str).unwrap_or("").to_string();
            let (namespace, name) = (text("namespace"), text("name"));
            (!namespace.is_empty() && !name.is_empty()).then(|| Found {
                id: format!("{namespace}.{name}"),
                name: Some(text("displayName")).filter(|d| !d.is_empty()).unwrap_or_else(|| name.clone()),
                description: text("description"),
                version: text("version"),
                downloads: e.get("downloadCount").and_then(Value::as_u64).unwrap_or(0),
            })
        })
        .collect())
}

/// The platform Open VSX names this computer by, for extensions built separately for each.
fn platform() -> String {
    let os = if cfg!(windows) {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "arm" => "armhf",
        "x86" => "ia32",
        other => other,
    };
    format!("{os}-{arch}")
}

/// The address of the .vsix to download from Open VSX's description of an extension: the one built for this
/// computer if it's built separately for each, else the one for all.
pub fn pick_download(text: &str, platform: &str) -> Result<String, String> {
    let json: Value = serde_json::from_str(text).map_err(|_| crate::i18n::tr("Open VSX's answer wasn't what was expected.").to_string())?;
    if let Some(problem) = json.get("error").and_then(Value::as_str) {
        return Err(crate::i18n::tf!("Open VSX says: {problem}"));
    }
    let downloads = json.get("downloads").and_then(Value::as_object);
    if let Some(downloads) = downloads.filter(|d| !d.is_empty() && !d.contains_key("universal")) {
        return downloads.get(platform).and_then(Value::as_str).map(str::to_string).ok_or_else(|| {
            let built: Vec<&str> = downloads.keys().map(String::as_str).collect();
            crate::i18n::tf!("It's built for {} only, not for this computer ({platform}).", built.join(", "))
        });
    }
    downloads
        .and_then(|d| d.get("universal"))
        .or_else(|| json.pointer("/files/download"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| crate::i18n::tr("Open VSX has no download for it.").to_string())
}

/// Downloads an extension from Open VSX by its id, namespace.name.
pub fn download(id: &str) -> Result<Vec<u8>, String> {
    let (namespace, name) = id.trim().split_once('.').filter(|(n, m)| !n.is_empty() && !m.is_empty()).ok_or_else(|| crate::i18n::tf!("\u{201C}{id}\u{201D} isn't an extension's id, which is written publisher.name."))?;
    let about = get(&format!("{OPEN_VSX}/api/{}/{}", encode(namespace), encode(name)), 16 * 1024 * 1024)?;
    let address = pick_download(&String::from_utf8_lossy(&about), &platform())?;
    get(&address, MOST_DOWNLOAD)
}

// ---------- The extensions in use ----------

static LOADED: RwLock<Option<Arc<Loaded>>> = RwLock::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// The names extension grammars were given by the highlighter, by scope, where a name was taken and changed.
static NAMES: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
/// Grammars the highlighter couldn't load, by extension.
static GRAMMAR_PROBLEMS: Mutex<Option<HashMap<String, Vec<String>>>> = Mutex::new(None);

/// What the extensions that are on give.
pub fn current() -> Arc<Loaded> {
    LOADED.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone().unwrap_or_default()
}

/// Goes up by one each time the extensions change, so what's been taken from them is taken again.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

fn set(loaded: Loaded) {
    *LOADED.write().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(loaded));
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Reads the extensions at start, with which are off and the icon theme from the settings.
pub fn start(settings: &str) {
    let value = |key: &str| settings.lines().filter_map(|l| l.split_once('=')).filter(|(k, _)| *k == key).map(|(_, v)| v.to_string()).collect::<Vec<_>>();
    let off: BTreeSet<String> = value("extension_off").into_iter().collect();
    let icons = value("file_icon_theme").into_iter().next().filter(|v| !v.is_empty());
    reload_with(&off, icons.as_deref());
}

fn reload_with(off: &BTreeSet<String>, icons: Option<&str>) {
    let loaded = folder().map_or_else(|| Loaded { off: off.clone(), icon_choice: icons.map(str::to_string), ..Loaded::default() }, |f| load_from(&f, off, icons));
    set(loaded);
}

/// Reads the extensions again, as they are now.
pub fn reload() {
    let now = current();
    reload_with(&now.off, now.icon_choice.as_deref());
}

pub fn set_on(id: &str, on: bool) {
    let now = current();
    let mut off = now.off.clone();
    if on {
        off.remove(id);
    } else {
        off.insert(id.to_string());
    }
    reload_with(&off, now.icon_choice.as_deref());
}

/// Chooses the file icon theme, by its key, or Limoniter's own badges for None.
pub fn set_icon_theme(key: Option<&str>) {
    let now = current();
    reload_with(&now.off, key);
}

pub fn to_settings() -> String {
    let now = current();
    let mut out = String::new();
    for id in &now.off {
        out += &format!("extension_off={id}\n");
    }
    if let Some(icons) = &now.icon_choice {
        out += &format!("file_icon_theme={icons}\n");
    }
    out
}

/// The grammars of the extensions that are on, for the highlighter: each loaded, and named so that no two
/// languages share a name (`taken` says which names are already in use).
pub fn grammars(taken: &HashSet<String>) -> Vec<syntect::parsing::SyntaxDefinition> {
    let (out, names, problems) = definitions(&current().grammars, taken);
    *NAMES.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(names);
    *GRAMMAR_PROBLEMS.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(problems);
    out
}

type Definitions = (Vec<syntect::parsing::SyntaxDefinition>, HashMap<String, String>, HashMap<String, Vec<String>>);

/// Grammars loaded, each with a name no other language has: the definitions, the names by scope, and what
/// couldn't be loaded, by extension.
fn definitions(grammars: &[LoadedGrammar], taken: &HashSet<String>) -> Definitions {
    let mut names = HashMap::new();
    let mut problems: HashMap<String, Vec<String>> = HashMap::new();
    let mut used = taken.clone();
    let mut out = Vec::new();
    for grammar in grammars {
        match syntect::parsing::SyntaxDefinition::load_from_str(&grammar.yaml, true, None) {
            Ok(mut definition) => {
                if used.contains(&definition.name) {
                    definition.name = format!("{} ({})", grammar.name, grammar.extension_name);
                }
                used.insert(definition.name.clone());
                names.insert(grammar.scope.clone(), definition.name.clone());
                out.push(definition);
            }
            Err(problem) => problems.entry(grammar.extension.clone()).or_default().push(crate::i18n::tf!("The grammar for {} couldn't be loaded: {problem}", grammar.name)),
        }
    }
    (out, names, problems)
}

/// What went wrong loading an extension's grammars into the highlighter.
pub fn grammar_problems(extension: &str) -> Vec<String> {
    GRAMMAR_PROBLEMS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().and_then(|p| p.get(extension).cloned()).unwrap_or_default()
}

impl LoadedLanguage {
    /// The name the language is shown by: the highlighter's for its grammar, if it was renamed.
    pub fn shown_name(&self) -> String {
        self.scope.as_ref().and_then(|scope| NAMES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().and_then(|n| n.get(scope).cloned())).unwrap_or_else(|| self.name.clone())
    }
}

/// Limoniter's name for a language VS Code knows by an id: an extension's language, or one in the table.
fn name_for_id_in(id: &str, languages: &[LoadedLanguage]) -> String {
    languages.iter().find(|l| l.id == id).map(LoadedLanguage::shown_name).or_else(|| LANGUAGE_IDS.iter().find(|(i, _)| *i == id).map(|(_, n)| n.to_string())).unwrap_or_else(|| id.to_string())
}

/// Whether a VS Code language id is the language Limoniter calls `name`.
fn id_is(id: &str, name: &str, languages: &[LoadedLanguage]) -> bool {
    let known = name_for_id_in(id, languages);
    known == name || known.eq_ignore_ascii_case(name) || id.eq_ignore_ascii_case(name)
}

/// VS Code's id for a language, by Limoniter's name for it, for a language server to be told.
pub fn language_id(name: &str) -> Option<String> {
    let now = current();
    now.languages.iter().find(|l| l.shown_name() == name).map(|l| l.id.clone())
}

/// The rules for a language, if an extension gives them.
pub fn rules_for(name: &str) -> Option<Arc<LanguageRules>> {
    let now = current();
    if now.languages.is_empty() {
        return None;
    }
    now.languages.iter().filter(|l| l.rules.is_some()).find(|l| l.shown_name() == name).and_then(|l| l.rules.clone())
}

/// The comment marks for a language an extension gives.
pub fn comment_marks(name: &str) -> Option<CommentMarks> {
    rules_for(name).and_then(|r| r.comments)
}

/// The snippets for a language (and those for every language).
pub fn snippets_for(name: &str) -> Vec<ExtSnippet> {
    let now = current();
    now.snippets.iter().filter(|s| s.languages.is_empty() || s.languages.iter().any(|id| id_is(id, name, &now.languages))).cloned().collect()
}

/// The command that starts the language server an extension gives for a language.
pub fn server_for(name: &str) -> Option<String> {
    let now = current();
    now.servers.iter().find(|s| s.languages.iter().any(|id| id_is(id, name, &now.languages))).map(|s| s.command.clone())
}

/// A syntax colour scheme from an extension's theme, by name.
pub fn scheme(name: &str) -> Option<Arc<syntect::highlighting::Theme>> {
    current().themes.iter().find(|t| t.document.syntax == name).map(|t| t.scheme.clone())
}

pub fn scheme_names() -> Vec<String> {
    current().themes.iter().map(|t| t.document.syntax.clone()).collect()
}

/// The picture for a file from the chosen icon theme, if one's chosen and has one for it.
pub fn icon_for(name: &str, light: bool) -> Option<PathBuf> {
    let now = current();
    let icons = now.icons.as_ref()?;
    let language = crate::highlight::engine().and_then(|engine| engine.detect(Some(Path::new(name)), "")).map(|syntax| crate::highlight::engine().map_or_else(String::new, |e| e.name_of(syntax).to_string()));
    let id = language.as_deref().and_then(|l| LANGUAGE_IDS.iter().find(|(_, n)| *n == l).map(|(i, _)| i.to_string()).or_else(|| now.languages.iter().find(|x| x.shown_name() == l).map(|x| x.id.clone())));
    icons.icon_for(name, id.as_deref(), light).map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zip with nothing compressed, as a .vsix is (compressed or not).
    pub(crate) fn stored_zip(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in files {
            let offset = out.len() as u32;
            let crc = crc32(data);
            let header = |sig: &[u8], central: bool| {
                let mut h = sig.to_vec();
                if central {
                    h.extend(20u16.to_le_bytes());
                }
                h.extend(20u16.to_le_bytes());
                h.extend([0u8; 4]);
                h.extend([0u8; 4]);
                h.extend(crc.to_le_bytes());
                h.extend((data.len() as u32).to_le_bytes());
                h.extend((data.len() as u32).to_le_bytes());
                h.extend((name.len() as u16).to_le_bytes());
                h.extend(0u16.to_le_bytes());
                if central {
                    // Comment length, disk, internal and external attributes.
                    h.extend([0u8; 10]);
                    h.extend(offset.to_le_bytes());
                }
                h.extend(name.as_bytes());
                h
            };
            out.extend(header(b"PK\x03\x04", false));
            out.extend(data);
            central.extend(header(b"PK\x01\x02", true));
        }
        let start = out.len() as u32;
        out.extend(&central);
        out.extend(b"PK\x05\x06");
        out.extend([0u8; 4]);
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((central.len() as u32).to_le_bytes());
        out.extend(start.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("limoniter-vsix-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn demo_vsix() -> Vec<u8> {
        let package = serde_json::json!({
            "name": "demo-lang", "publisher": "Someone", "displayName": "%display%", "version": "1.2.3",
            "contributes": {
                "languages": [{ "id": "demo", "aliases": ["Demo Language", "demo"], "extensions": [".dmo"], "configuration": "./language-configuration.json" }],
                "grammars": [{ "language": "demo", "scopeName": "source.demo", "path": "./syntaxes/demo.tmLanguage.json" }, { "scopeName": "injection.demo", "path": "./syntaxes/demo.tmLanguage.json", "injectTo": ["source.js"] }],
                "themes": [{ "label": "Demo Night", "uiTheme": "vs-dark", "path": "./themes/night.json" }],
                "snippets": [{ "language": "demo", "path": "./snippets.json" }],
                "iconThemes": [{ "id": "demo-icons", "label": "Demo Icons", "path": "./icons/theme.json" }]
            }
        });
        let grammar = serde_json::json!({ "scopeName": "source.demo", "patterns": [{ "match": "\\bdemo\\b", "name": "keyword.demo" }] });
        let config = "{ // comments are allowed\n \"comments\": { \"lineComment\": \"--\", \"blockComment\": [\"{-\", \"-}\"] }, \"brackets\": [[\"(\", \")\"]], \"autoClosingPairs\": [[\"(\", \")\"], { \"open\": \"'\", \"close\": \"'\" }], \"indentationRules\": { \"increaseIndentPattern\": \"\\\\bdo\\\\s*$\", \"decreaseIndentPattern\": \"^\\\\s*end\\\\b\" }, }";
        let theme = r##"{ "colors": { "editor.background": "#101010", "editor.foreground": "#e0e0e0" }, "tokenColors": [{ "scope": "keyword", "settings": { "foreground": "#ff0000" } }] }"##;
        let snippets = serde_json::json!({ "Loop": { "prefix": "loop", "body": ["loop ${1:i} do", "\t$0", "end"], "description": "A loop" } });
        let icons = serde_json::json!({ "iconDefinitions": { "d": { "iconPath": "./d.svg" }, "f": { "iconPath": "./f.svg" }, "bad": { "iconPath": "../../../../etc/passwd" } }, "file": "f", "fileExtensions": { "dmo": "d" } });
        stored_zip(&[
            ("extension.vsixmanifest", b"<x/>".to_vec()),
            ("extension/package.json", package.to_string().into_bytes()),
            ("extension/package.nls.json", br#"{ "display": "Demo for Tests" }"#.to_vec()),
            ("extension/syntaxes/demo.tmLanguage.json", grammar.to_string().into_bytes()),
            ("extension/language-configuration.json", config.as_bytes().to_vec()),
            ("extension/themes/night.json", theme.as_bytes().to_vec()),
            ("extension/snippets.json", snippets.to_string().into_bytes()),
            ("extension/icons/theme.json", icons.to_string().into_bytes()),
            ("extension/icons/d.svg", b"<svg xmlns='http://www.w3.org/2000/svg' width='16' height='16'/>".to_vec()),
            ("extension/icons/f.svg", b"<svg xmlns='http://www.w3.org/2000/svg' width='16' height='16'/>".to_vec()),
            ("extension/../escaped.txt", b"no".to_vec()),
        ])
    }

    #[test]
    fn a_declarative_extension_is_installed_and_everything_it_gives_is_taken_in() {
        let folder = scratch("install");
        let installed = install(demo_vsix(), &folder).unwrap();
        assert_eq!(installed.manifest.id, "someone.demo-lang");
        assert_eq!(installed.manifest.name, "Demo for Tests", "from package.nls.json");
        assert!(folder.join("someone.demo-lang/package.json").is_file());
        assert!(!folder.join("escaped.txt").exists() && !folder.join("someone.demo-lang/../escaped.txt").exists());
        assert!(installed.gave.iter().any(|g| g.contains("Demo Night")), "{:?}", installed.gave);
        assert!(installed.gave.iter().any(|g| g.contains("Colouring for Demo Language (.dmo)")), "{:?}", installed.gave);
        assert!(installed.problems.iter().any(|p| p.contains("only add to other languages")), "the injection grammar is said to be left out");

        let loaded = load_from(&folder, &BTreeSet::new(), Some("someone.demo-lang/demo-icons"));
        assert_eq!(loaded.extensions.len(), 1);
        assert_eq!(loaded.themes.len(), 1);
        assert_eq!(loaded.themes[0].document.editor_back, eframe::egui::Color32::from_rgb(16, 16, 16));
        assert_eq!(loaded.grammars.len(), 1);
        let definition = syntect::parsing::SyntaxDefinition::load_from_str(&loaded.grammars[0].yaml, true, None).unwrap();
        assert_eq!(definition.name, "Demo Language");
        assert_eq!(definition.file_extensions, vec!["dmo".to_string()]);
        let rules = loaded.languages[0].rules.clone().unwrap();
        assert_eq!(rules.comments, Some(CommentMarks { line: Some("--"), block: Some(("{-", "-}")) }));
        assert_eq!(loaded.snippets.len(), 1);
        assert_eq!(loaded.snippets[0].languages, vec!["demo".to_string()]);
        let icons = loaded.icons.as_ref().unwrap();
        assert!(icons.icon_for("x.dmo", None, false).unwrap().ends_with("d.svg"));
        assert!(icons.icon_for("x.txt", None, false).unwrap().ends_with("f.svg"), "the theme's picture for any file");
        assert!(icons.definitions.values().all(|p| p.starts_with(&folder)), "nothing outside the extension");

        // Turned off, it gives nothing, but is still listed.
        let off: BTreeSet<String> = ["someone.demo-lang".to_string()].into();
        let loaded = load_from(&folder, &off, None);
        assert_eq!(loaded.extensions.len(), 1);
        assert!(!loaded.extensions[0].on);
        assert!(loaded.themes.is_empty() && loaded.grammars.is_empty() && loaded.snippets.is_empty() && loaded.languages.is_empty());

        remove(&loaded.extensions[0]).unwrap();
        assert!(load_from(&folder, &BTreeSet::new(), None).extensions.is_empty());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_extension_that_is_only_its_program_is_turned_away_plainly() {
        let folder = scratch("program");
        let package = serde_json::json!({ "name": "lens", "publisher": "p", "displayName": "Lens", "main": "./dist/lens.js", "contributes": { "commands": [{ "command": "a" }, { "command": "b" }] } });
        let vsix = stored_zip(&[("extension/package.json", package.to_string().into_bytes()), ("extension/dist/lens.js", b"exports.activate = () => {}".to_vec())]);
        let problem = install(vsix, &folder).unwrap_err();
        assert!(problem.contains("Lens works through its own program"), "{problem}");
        assert!(std::fs::read_dir(&folder).unwrap().next().is_none(), "nothing is left behind");
        assert!(install(b"not a zip at all".to_vec(), &folder).is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_extension_with_a_program_says_what_is_left_behind() {
        let folder = scratch("partial");
        let package = serde_json::json!({ "name": "vscode-zig", "publisher": "ziglang", "displayName": "Zig", "main": "./out/extension", "dependencies": { "vscode-languageclient": "9" },
            "contributes": { "languages": [{ "id": "zig", "aliases": ["Zig"], "extensions": [".zig"] }], "grammars": [{ "language": "zig", "scopeName": "source.zig", "path": "./zig.json" }], "commands": [{ "command": "a" }],
                "configuration": { "properties": { "zig.path": {}, "zig.zls.path": {} } } } });
        let grammar = serde_json::json!({ "scopeName": "source.zig", "patterns": [] });
        let vsix = stored_zip(&[("extension/package.json", package.to_string().into_bytes()), ("extension/zig.json", grammar.to_string().into_bytes()), ("extension/out/extension.js", b"new LanguageClient('zls')".to_vec())]);
        let installed = install(vsix, &folder).unwrap();
        assert!(installed.problems.iter().any(|p| p.contains("1 commands, 2 settings")), "{:?}", installed.problems);
        assert!(installed.gave.iter().any(|g| g.contains("A language server for Zig: zls")), "a server it's known to use: {:?}", installed.gave);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_node_server_it_carries_is_found() {
        let folder = scratch("node");
        std::fs::create_dir_all(folder.join("out")).unwrap();
        let package = serde_json::json!({ "name": "m68k", "publisher": "g", "main": "./out/extension", "dependencies": { "vscode-languageclient": "9" }, "contributes": { "languages": [{ "id": "m68k", "aliases": ["M68k"] }] } });
        std::fs::write(folder.join("package.json"), package.to_string()).unwrap();
        std::fs::write(folder.join("out/extension.js"), "let s = path.join('out', 'server.js');").unwrap();
        std::fs::write(folder.join("out/server.js"), "connection.listen()").unwrap();
        let manifest = read_manifest(&folder).unwrap();
        let server = find_server(&folder, &manifest).unwrap();
        assert_eq!(server.program, "node");
        assert!(server.command.starts_with("node \"") && server.command.ends_with("server.js\" --stdio"), "{}", server.command);
        assert_eq!(server.languages, vec!["m68k".to_string()]);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn open_vsx_s_answers_are_read() {
        let search = r#"{"offset":0,"totalSize":1,"extensions":[{"namespace":"dracula-theme","name":"theme-dracula","version":"2.25.1","displayName":"Dracula Theme Official","description":"A dark theme","downloadCount":471103}]}"#;
        let found = parse_search(search).unwrap();
        assert_eq!(found[0].id, "dracula-theme.theme-dracula");
        assert_eq!(found[0].name, "Dracula Theme Official");
        assert_eq!(found[0].downloads, 471_103);
        let universal = r#"{"files":{"download":"https://x/u.vsix"},"downloads":{"universal":"https://x/u.vsix"}}"#;
        assert_eq!(pick_download(universal, "linux-x64").unwrap(), "https://x/u.vsix");
        let each = r#"{"files":{"download":"https://x/darwin.vsix"},"downloads":{"darwin-arm64":"https://x/darwin.vsix","linux-x64":"https://x/linux.vsix"}}"#;
        assert_eq!(pick_download(each, "linux-x64").unwrap(), "https://x/linux.vsix", "the one built for this computer");
        assert!(pick_download(each, "win32-arm64").unwrap_err().contains("darwin-arm64"));
        assert!(pick_download(r#"{"error":"Extension not found: x.y"}"#, "linux-x64").unwrap_err().contains("not found"));
        assert_eq!(encode("a b/c"), "a%20b%2Fc");
    }

    fn fill() -> Fill<'static> {
        use chrono::TimeZone;
        Fill { date: chrono::Local.with_ymd_and_hms(2026, 11, 20, 9, 5, 0).unwrap(), file: "main.rs", selection: "picked" }
    }

    #[test]
    fn vs_code_snippets_are_filled_in() {
        let expand = |body: &str| expand_snippet(body, &fill());
        assert_eq!(expand("for ${1:i} in ${2:list} {\n\t$0\n}"), ("for i in list {\n\t".to_string(), "\n}".to_string()));
        assert_eq!(expand("print($1)$2"), ("print(".to_string(), ")".to_string()), "no $0: the first stop");
        assert_eq!(expand("${1|one,two|} $TM_FILENAME_BASE ${CURRENT_YEAR}"), (String::new(), "one main 2026".to_string()), "at the first stop");
        assert_eq!(expand("[${TM_SELECTED_TEXT:nothing}]"), ("[picked]".to_string(), String::new()));
        assert_eq!(expand("${UNKNOWN_THING:fallback} \\$5 a\\}b"), ("fallback $5 a}b".to_string(), String::new()));
        assert_eq!(expand("${1:outer ${2:inner}} x$0"), ("outer inner x".to_string(), String::new()));
        assert_eq!(expand("${TM_FILENAME/(.*)\\..+$/$1/} done"), ("main.rs done".to_string(), String::new()));
    }

    #[test]
    fn snippets_files_give_their_names_prefixes_and_languages() {
        let json = serde_json::json!({ "Log": { "prefix": ["log", "clg"], "body": "console.log($1);", "description": "Log it" }, "Any": { "prefix": "x", "body": ["a", "b"], "scope": "javascript, typescript" }, "Broken": { "prefix": "y" } });
        let snippets = read_snippets(&json, None, "ext");
        assert_eq!(snippets.len(), 2);
        let log = snippets.iter().find(|s| s.name == "Log").unwrap();
        assert_eq!(log.prefix, "log, clg");
        assert!(log.languages.is_empty());
        let any = snippets.iter().find(|s| s.name == "Any").unwrap();
        assert_eq!(any.body, "a\nb");
        assert_eq!(any.languages, vec!["javascript".to_string(), "typescript".to_string()]);
        assert!(id_is("javascript", "JavaScript", &[]) && id_is("cpp", "C++", &[]) && !id_is("cpp", "C", &[]));
    }

    #[test]
    fn language_configuration_closes_pairs_and_indents() {
        let json = crate::vsix_grammar::parse_jsonc(r#"{ "autoClosingPairs": [["{", "}"], ["/**", " */"], {"open": "/*", "close": "*/"}, {"open": "\"", "close": "\"", "notIn": ["string"]}],
            "surroundingPairs": [["(", ")"]], "indentationRules": { "increaseIndentPattern": {"pattern": "^.*\\{[^}]*$"}, "decreaseIndentPattern": "^\\s*\\}" } }"#).unwrap();
        let rules = read_rules(&json);
        assert_eq!(rules.closing_for("fn x() {", ""), Some("}"));
        assert_eq!(rules.closing_for("fn x() {", " "), Some("}"));
        assert_eq!(rules.closing_for("fn x() {", "abc"), None, "not before a word");
        assert_eq!(rules.closing_for("/**", ""), Some(" */"), "the longest opener");
        assert_eq!(rules.closing_for("say \"", ""), Some("\""));
        assert_eq!(rules.closing_for("it\"", ""), None, "a quote after a letter");
        assert_eq!(rules.surrounding("("), Some(")"));
        assert!(rules.is_closer("}"));
        assert!(rules.indents_after("if x {"));
        assert!(!rules.indents_after("if x { y }"));
        assert!(rules.outdents("    }"));
        assert!(rules.comments.is_none());
    }

    #[test]
    fn a_grammar_whose_name_is_taken_is_named_after_its_extension_and_wins_its_endings() {
        let grammar = |name: &str, scope: &str, yaml: &str| LoadedGrammar { extension: "x.y".into(), extension_name: "Zig Tools".into(), name: name.into(), scope: scope.into(), yaml: yaml.into() };
        let good = crate::vsix_grammar::convert(&serde_json::json!({"scopeName": "source.zig", "patterns": [{"match": "fn", "name": "keyword"}]}), &crate::vsix_grammar::GrammarInfo { name: "Zig", endings: &["zig".to_string()], first_line: None }).unwrap();
        let taken: HashSet<String> = ["Zig".to_string()].into();
        let (out, names, problems) = definitions(&[grammar("Zig", "source.zig", &good.yaml), grammar("Broken", "source.b", "name: [")], &taken);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "Zig (Zig Tools)");
        assert_eq!(names["source.zig"], "Zig (Zig Tools)");
        assert!(problems["x.y"][0].contains("Broken"));
        // Added after another grammar for the same ending, it's the one found for it.
        let mut builder = syntect::parsing::SyntaxSetBuilder::new();
        let other = crate::vsix_grammar::convert(&serde_json::json!({"scopeName": "source.zig.old", "patterns": []}), &crate::vsix_grammar::GrammarInfo { name: "Zig", endings: &["zig".to_string()], first_line: None }).unwrap();
        builder.add(syntect::parsing::SyntaxDefinition::load_from_str(&other.yaml, true, None).unwrap());
        builder.add(out.into_iter().next().unwrap());
        let set = builder.build();
        assert_eq!(set.find_syntax_by_extension("zig").unwrap().name, "Zig (Zig Tools)");
    }

    #[test]
    fn paths_never_lead_out_of_the_extension() {
        let root = Path::new("/ext/x");
        assert_eq!(inside(root, root, "./a/b.json"), Some(PathBuf::from("/ext/x/a/b.json")));
        assert_eq!(inside(root, &root.join("dist"), "./../icons/a.svg"), Some(PathBuf::from("/ext/x/icons/a.svg")));
        assert_eq!(inside(root, root, "../y/z"), None);
        assert_eq!(inside(root, root, "/etc/passwd"), None);
    }

    #[test]
    fn real_extensions_from_open_vsx_install() {
        // Downloaded once from Open VSX into this folder, if it's there; the test passes over them otherwise.
        let Some(dir) = std::env::var_os("LIMONITER_TEST_VSIX").map(PathBuf::from) else { return };
        let folder = scratch("real");
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "vsix") {
                continue;
            }
            match install(std::fs::read(&path).unwrap(), &folder) {
                Ok(installed) => eprintln!("{}: {} gives {:?}; problems {:?}", file_name(&path), installed.manifest.name, installed.gave, installed.problems),
                Err(problem) => eprintln!("{}: refused: {problem}", file_name(&path)),
            }
        }
        let loaded = load_from(&folder, &BTreeSet::new(), None);
        for grammar in &loaded.grammars {
            let result = syntect::parsing::SyntaxDefinition::load_from_str(&grammar.yaml, true, None);
            eprintln!("grammar {}: {}", grammar.name, result.as_ref().map_or_else(|e| e.to_string(), |_| "loads".to_string()));
        }
        let _ = std::fs::remove_dir_all(&folder);
    }
}
