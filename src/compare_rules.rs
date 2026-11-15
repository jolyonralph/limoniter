//! Rules for how files are compared, by name: text with what's to be left out (spaces, case, line endings, lines
//! matching a pattern such as a timestamp), bytes, pictures, size and time only, or not compared at all. The first
//! rule that matches a file decides; a file no rule matches is compared as it always was. They're kept in the
//! settings, and a saved comparison can have its own.

use crate::diff_engine::{TextOptions, glob_match, line_key, patterns};
use serde_json::{Value, json};

/// How a file a rule matches is compared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RuleKind {
    /// As text, with what the rule says to leave out.
    #[default]
    Text,
    /// Byte by byte, and shown as bytes when opened.
    Binary,
    /// As pictures: two files that show the same pixels are the same.
    Picture,
    /// By size and modified time only, never reading them, even when comparing by contents.
    SizeAndTime,
    /// Left out of comparisons altogether.
    Skip,
}

impl RuleKind {
    pub const ALL: [RuleKind; 5] = [RuleKind::Text, RuleKind::Binary, RuleKind::Picture, RuleKind::SizeAndTime, RuleKind::Skip];

    pub fn words(self) -> &'static str {
        match self {
            RuleKind::Text => "Text",
            RuleKind::Binary => "Bytes",
            RuleKind::Picture => "Picture",
            RuleKind::SizeAndTime => "Size and time only",
            RuleKind::Skip => "Always leave out",
        }
    }

    fn id(self) -> &'static str {
        match self {
            RuleKind::Text => "text",
            RuleKind::Binary => "binary",
            RuleKind::Picture => "picture",
            RuleKind::SizeAndTime => "size-time",
            RuleKind::Skip => "skip",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        RuleKind::ALL.into_iter().find(|kind| kind.id() == id)
    }
}

/// One rule: which files, and how they're compared.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rule {
    /// Patterns separated by ; or commas. One ending / is for folders only, as in "target/"; one with a / inside
    /// is matched against the path from the top, any other against the name.
    pub patterns: String,
    pub kind: RuleKind,
    /// For text: spaces, case and line endings.
    pub text: TextOptions,
    /// For text: lines matching this regular expression count as the same as each other, such as a line with a
    /// date in it. Empty for none.
    pub ignore_lines: String,
}

impl Rule {
    pub fn new(kind: RuleKind, patterns: &str) -> Self {
        Self { patterns: patterns.to_string(), kind, ..Self::default() }
    }

    /// Whether it matches a file or folder.
    pub fn matches(&self, path: &str, name: &str, is_dir: bool) -> bool {
        patterns(&self.patterns).iter().any(|pattern| pattern_matches(pattern, path, name, is_dir))
    }

    /// Whether, as text, it leaves anything out, so files that differ in bytes may still be the same.
    pub fn loosens_text(&self) -> bool {
        self.kind == RuleKind::Text && (self.text.ignore_whitespace || self.text.ignore_case || self.text.ignore_line_endings || !self.ignore_lines.trim().is_empty())
    }

    /// Its pattern for lines to leave out, compiled, or why it can't be.
    pub fn ignore_regex(&self) -> Result<Option<regex::Regex>, String> {
        let text = self.ignore_lines.trim();
        if text.is_empty() {
            return Ok(None);
        }
        regex::Regex::new(text).map(Some).map_err(|e| format!("\u{201C}{text}\u{201D} isn't a pattern that can be used: {e}"))
    }

    /// What it does, in a few words, for a list.
    pub fn describe(&self) -> String {
        let mut said = self.kind.words().to_string();
        if self.kind == RuleKind::Text {
            let mut leaving = Vec::new();
            if self.text.ignore_whitespace {
                leaving.push("spaces");
            }
            if self.text.ignore_case {
                leaving.push("case");
            }
            if self.text.ignore_line_endings {
                leaving.push("line endings");
            }
            if !self.ignore_lines.trim().is_empty() {
                leaving.push("some lines");
            }
            if !leaving.is_empty() {
                said.push_str(&format!(", leaving out {}", leaving.join(", ")));
            }
        }
        said
    }

    fn to_json(&self) -> Value {
        json!({
            "patterns": self.patterns,
            "kind": self.kind.id(),
            "ignore_whitespace": self.text.ignore_whitespace,
            "ignore_case": self.text.ignore_case,
            "ignore_line_endings": self.text.ignore_line_endings,
            "ignore_lines": self.ignore_lines,
        })
    }

    fn from_json(value: &Value) -> Option<Self> {
        let flag = |name: &str| value.get(name).and_then(Value::as_bool).unwrap_or(false);
        Some(Self {
            patterns: value.get("patterns")?.as_str()?.to_string(),
            kind: RuleKind::from_id(value.get("kind")?.as_str()?)?,
            text: TextOptions { ignore_whitespace: flag("ignore_whitespace"), ignore_case: flag("ignore_case"), ignore_line_endings: flag("ignore_line_endings") },
            ignore_lines: value.get("ignore_lines").and_then(Value::as_str).unwrap_or_default().to_string(),
        })
    }
}

/// Whether one pattern matches: "name/" is a folder by that name, "a/b" a path from the top, "*.o" a name.
fn pattern_matches(pattern: &str, path: &str, name: &str, is_dir: bool) -> bool {
    match pattern.strip_suffix('/') {
        Some(folder) => is_dir && if folder.contains('/') { glob_match(folder.trim_start_matches('/'), path, true) } else { glob_match(folder, name, true) },
        None if pattern.contains('/') => glob_match(pattern.trim_start_matches('/'), path, true),
        None => glob_match(pattern, name, true),
    }
}

/// The rules, in order: the first that matches decides.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rules {
    pub list: Vec<Rule>,
}

impl Rules {
    /// The rules Limoniter starts with: build output and version control folders left out, pictures compared as
    /// pictures, programs and other binaries byte by byte.
    pub fn usual() -> Self {
        Self {
            list: vec![
                Rule::new(RuleKind::Skip, "*.o; *.obj; *.pyc; target/; .git/; .svn/; .hg/; node_modules/; __pycache__/"),
                Rule::new(RuleKind::Picture, "*.png; *.gif; *.bmp; *.jpg; *.jpeg; *.iff; *.ilbm; *.lbm; *.pcx; *.tga; *.scr; *.koa; *.neo; *.pi1; *.pc1"),
                Rule::new(RuleKind::Binary, "*.exe; *.dll; *.so; *.dylib; *.bin; *.rom; *.zip; *.7z; *.gz; *.tap; *.tzx; *.d64; *.adf; *.prg"),
            ],
        }
    }

    /// The rule for a file or folder, if one matches.
    pub fn rule_for(&self, path: &str, name: &str, is_dir: bool) -> Option<&Rule> {
        self.list.iter().find(|rule| rule.matches(path, name, is_dir))
    }

    /// Whether a file or folder is left out.
    pub fn skips(&self, path: &str, name: &str, is_dir: bool) -> bool {
        self.rule_for(path, name, is_dir).is_some_and(|rule| rule.kind == RuleKind::Skip)
    }

    /// The first pattern for lines to leave out that can't be used, as a problem to show.
    pub fn problem(&self) -> Option<String> {
        self.list.iter().find_map(|rule| rule.ignore_regex().err())
    }

    pub fn to_json(&self) -> Value {
        Value::Array(self.list.iter().map(Rule::to_json).collect())
    }

    pub fn from_json(value: &Value) -> Self {
        Self { list: value.as_array().map_or_else(Vec::new, |items| items.iter().filter_map(Rule::from_json).collect()) }
    }

    /// As the settings file keeps them: nothing while they're the usual ones.
    pub fn to_settings(&self) -> String {
        if *self == Self::usual() {
            return String::new();
        }
        format!("compare_rules={}\n", self.to_json())
    }

    /// From the settings file's text.
    pub fn from_settings(settings: &str) -> Self {
        settings.lines().find_map(|line| line.strip_prefix("compare_rules=")).and_then(|text| serde_json::from_str::<Value>(text).ok()).map_or_else(Self::usual, |value| Self::from_json(&value))
    }
}

/// The keys two texts are compared by under a rule: each line's, with lines matching its pattern all alike.
pub fn text_keys(text: &str, options: &TextOptions, ignore: Option<&regex::Regex>) -> Vec<u64> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if text.ends_with('\n') || text.is_empty() {
        lines.pop();
    }
    lines.iter().map(|line| if ignore.is_some_and(|re| re.is_match(line.trim_end_matches('\r'))) { 0 } else { line_key::<u8>(line, None, options) }).collect()
}

/// Whether two files' bytes are the same text under a rule.
pub fn same_text(a: &[u8], b: &[u8], rule: &Rule, ignore: Option<&regex::Regex>) -> Option<bool> {
    if crate::codec::looks_binary(a) || crate::codec::looks_binary(b) {
        return None;
    }
    let (a, b) = (String::from_utf8_lossy(a), String::from_utf8_lossy(b));
    let mut options = rule.text;
    if !options.ignore_line_endings {
        // Without endings left out, a last line with no ending differs from one with.
        if a.ends_with('\n') != b.ends_with('\n') {
            return Some(false);
        }
    } else {
        options.ignore_line_endings = true;
    }
    Some(text_keys(&a, &options, ignore) == text_keys(&b, &options, ignore))
}

/// Whether two files show the same picture.
pub fn same_picture(name: &str, a: &[u8], b: &[u8]) -> Option<bool> {
    let decode = |bytes: &[u8]| crate::pictures::detect(name, bytes).and_then(|format| crate::pictures::decode_named(format, name, bytes).ok());
    let (pa, pb) = (decode(a)?, decode(b)?);
    Some(pa.width == pb.width && pa.height == pb.height && pa.rgba == pb.rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_rule_that_matches_decides() {
        let rules = Rules::usual();
        assert!(rules.skips("src/main.o", "main.o", false));
        assert!(rules.skips("target", "target", true));
        assert!(rules.skips("a/b/.git", ".git", true));
        assert!(!rules.skips("target", "target", false), "target/ is for folders only");
        assert!(!rules.skips("src/main.rs", "main.rs", false));
        assert_eq!(rules.rule_for("art/logo.PNG", "logo.PNG", false).map(|rule| rule.kind), Some(RuleKind::Picture));
        assert_eq!(rules.rule_for("game.prg", "game.prg", false).map(|rule| rule.kind), Some(RuleKind::Binary));
        assert!(rules.rule_for("notes.txt", "notes.txt", false).is_none());
        let by_path = Rule::new(RuleKind::SizeAndTime, "logs/*.log");
        assert!(by_path.matches("logs/today.log", "today.log", false));
        assert!(!by_path.matches("other/today.log", "today.log", false));
    }

    #[test]
    fn text_rules_leave_out_what_they_say() {
        let mut rule = Rule::new(RuleKind::Text, "*.txt");
        assert!(!rule.loosens_text());
        assert_eq!(same_text(b"a\nb\n", b"a\nb\n", &rule, None), Some(true));
        assert_eq!(same_text(b"a\r\nb\r\n", b"a\nb\n", &rule, None), Some(false));
        rule.text.ignore_line_endings = true;
        assert!(rule.loosens_text());
        assert_eq!(same_text(b"a\r\nb\r\n", b"a\nb\n", &rule, None), Some(true));
        rule.text.ignore_whitespace = true;
        rule.text.ignore_case = true;
        assert_eq!(same_text(b"Hello  World\n", b"helloworld\n", &rule, None), Some(true));
        // Lines with a time in them count as the same as each other.
        rule.ignore_lines = r"^Built at \d+:\d+".into();
        let re = rule.ignore_regex().unwrap();
        assert_eq!(same_text(b"x\nBuilt at 10:31\ny\n", b"x\nBuilt at 22:02\ny\n", &rule, re.as_ref()), Some(true));
        assert_eq!(same_text(b"x\nBuilt at 10:31\ny\n", b"x\nBuilt at 22:02\nz\n", &rule, re.as_ref()), Some(false));
        assert_eq!(same_text(b"\0\x01binary", b"text", &rule, None), None, "bytes aren't text");
        rule.ignore_lines = "(".into();
        assert!(rule.ignore_regex().is_err());
        assert!(Rules { list: vec![rule] }.problem().is_some());
    }

    #[test]
    fn rules_are_kept_in_the_settings_only_when_changed() {
        assert_eq!(Rules::usual().to_settings(), "");
        assert_eq!(Rules::from_settings(""), Rules::usual());
        let mut rules = Rules::usual();
        rules.list.push(Rule { patterns: "*.log".into(), kind: RuleKind::Text, text: TextOptions { ignore_whitespace: true, ..Default::default() }, ignore_lines: r"^\d{4}-".into() });
        let kept = rules.to_settings();
        assert!(kept.starts_with("compare_rules=") && kept.ends_with('\n') && kept.lines().count() == 1);
        assert_eq!(Rules::from_settings(&format!("theme=dark\n{kept}")), rules);
        // An emptied list stays empty.
        let none = Rules::default().to_settings();
        assert_eq!(Rules::from_settings(&none), Rules::default());
        assert_eq!(Rules::from_json(&Rules::usual().to_json()), Rules::usual());
    }
}
