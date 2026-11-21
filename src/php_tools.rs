//! PHP tools, the easy ones: the chores of keeping old PHP working. Short open tags made long, the causes of
//! "headers already sent" taken away, `array()` written as `[]`, a check for what PHP 7 and 8 removed (with the
//! safe ones fixed), old Latin-1 and Windows-1252 files made UTF-8, and what `php -l` says read back by line.
//!
//! Everything here works on a file's bytes as they are, so it never needs to know the file's encoding: all of
//! PHP's own syntax is plain ASCII, and the changes only ever put ASCII in. The file is read the way PHP reads it,
//! well enough to know which parts are PHP code and which are text around it, strings, heredocs or comments, so a
//! change is only ever made to the code itself.

use std::ops::Range;

/// What a piece of a PHP file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Text outside PHP's tags, sent to the browser as it is.
    Html,
    /// `<?php`, `<?` or `<?=`.
    Open(Open),
    /// `?>`.
    Close,
    Ident,
    /// A variable, `$name`.
    Var,
    /// A string in quotes or backticks.
    Str,
    /// A heredoc or nowdoc, from `<<<` to its closing name.
    Heredoc,
    Comment,
    Space,
    Number,
    /// Punctuation: one character, or `->`, `?->`, `::`, `=>` or `#[`.
    Punct,
}

/// The kinds of opening tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Open {
    /// `<?php`.
    Long,
    /// `<?`, which only works where short_open_tag is on.
    Short,
    /// `<?=`, which always works.
    Echo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
}

impl Token {
    fn text<'a>(&self, src: &'a [u8]) -> &'a [u8] {
        &src[self.start..self.end]
    }

    fn is(&self, src: &[u8], kind: Kind, text: &str) -> bool {
        self.kind == kind && self.text(src).eq_ignore_ascii_case(text.as_bytes())
    }
}

fn ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

fn ident_char(b: u8) -> bool {
    ident_start(b) || b.is_ascii_digit()
}

fn starts_with_ci(src: &[u8], at: usize, word: &[u8]) -> bool {
    src.len() >= at + word.len() && src[at..at + word.len()].eq_ignore_ascii_case(word)
}

/// The opening tag at `at` (which starts `<?`), and its length, or None where it's text: `<?xml` and the like,
/// which an XML file or a page that writes XML has, are left as text, as they would be with short tags off.
fn open_tag(src: &[u8], at: usize) -> Option<(Open, usize)> {
    if starts_with_ci(src, at, b"<?php") && src.get(at + 5).is_none_or(|b| b.is_ascii_whitespace()) {
        return Some((Open::Long, 5));
    }
    if src.get(at + 2) == Some(&b'=') {
        return Some((Open::Echo, 3));
    }
    if starts_with_ci(src, at, b"<?xml") {
        return None;
    }
    Some((Open::Short, 2))
}

/// Splits a file into its pieces, as PHP reads it with short tags on.
pub fn tokens(src: &[u8]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0;
    let n = src.len();
    let push = |out: &mut Vec<Token>, kind, start, end| {
        if end > start {
            out.push(Token { kind, start, end });
        }
    };
    'html: while i < n {
        // Outside PHP: text up to the next opening tag.
        let start = i;
        let mut tag = None;
        while i + 1 < n {
            if src[i] == b'<' && src[i + 1] == b'?' {
                if let Some(found) = open_tag(src, i) {
                    tag = Some(found);
                    break;
                }
                i += 2;
                continue;
            }
            i += 1;
        }
        let Some((open, length)) = tag else {
            push(&mut out, Kind::Html, start, n);
            break;
        };
        push(&mut out, Kind::Html, start, i);
        push(&mut out, Kind::Open(open), i, i + length);
        i += length;
        // Inside PHP, up to ?> or the end.
        while i < n {
            let b = src[i];
            let next = src.get(i + 1).copied().unwrap_or(0);
            let start = i;
            if b == b'?' && next == b'>' {
                push(&mut out, Kind::Close, i, i + 2);
                i += 2;
                continue 'html;
            }
            if b.is_ascii_whitespace() {
                while i < n && src[i].is_ascii_whitespace() {
                    i += 1;
                }
                push(&mut out, Kind::Space, start, i);
            } else if b == b'#' && next == b'[' {
                // An attribute, in PHP 8: code, not a comment.
                i += 2;
                push(&mut out, Kind::Punct, start, i);
            } else if b == b'#' || (b == b'/' && next == b'/') {
                // A line comment ends at the line's end, or at ?>, which still closes PHP.
                while i < n && src[i] != b'\n' && !(src[i] == b'?' && src.get(i + 1) == Some(&b'>')) {
                    i += 1;
                }
                // The line's ending is left out of it.
                let mut end = i;
                if end > start && src[end - 1] == b'\r' && src.get(end) == Some(&b'\n') {
                    end -= 1;
                }
                push(&mut out, Kind::Comment, start, end);
                push(&mut out, Kind::Space, end, i);
            } else if b == b'/' && next == b'*' {
                i += 2;
                while i < n && !(src[i] == b'*' && src.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                i = (i + 2).min(n);
                push(&mut out, Kind::Comment, start, i);
            } else if b == b'\'' || b == b'"' || b == b'`' {
                i += 1;
                while i < n && src[i] != b {
                    i += if src[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 1).min(n);
                push(&mut out, Kind::Str, start, i);
            } else if b == b'<' && src[i..].starts_with(b"<<<") && let Some(end) = heredoc_end(src, i) {
                i = end;
                push(&mut out, Kind::Heredoc, start, i);
            } else if b == b'$' && src.get(i + 1).is_some_and(|c| ident_start(*c)) {
                i += 1;
                while i < n && ident_char(src[i]) {
                    i += 1;
                }
                push(&mut out, Kind::Var, start, i);
            } else if ident_start(b) {
                while i < n && ident_char(src[i]) {
                    i += 1;
                }
                push(&mut out, Kind::Ident, start, i);
            } else if b.is_ascii_digit() {
                while i < n && (src[i].is_ascii_alphanumeric() || src[i] == b'_' || (src[i] == b'.' && src.get(i + 1).is_some_and(u8::is_ascii_digit))) {
                    i += 1;
                }
                push(&mut out, Kind::Number, start, i);
            } else {
                let wide = [&b"?->"[..], b"->", b"::", b"=>"].into_iter().find(|op| src[i..].starts_with(op)).map_or(1, <[u8]>::len);
                i += wide;
                push(&mut out, Kind::Punct, start, i);
            }
        }
    }
    out
}

/// Where a heredoc or nowdoc starting at `at` (`<<<`) ends: just after its closing name. None if it isn't one.
fn heredoc_end(src: &[u8], at: usize) -> Option<usize> {
    let n = src.len();
    let mut i = at + 3;
    while i < n && (src[i] == b' ' || src[i] == b'\t') {
        i += 1;
    }
    let quote = src.get(i).copied().filter(|q| *q == b'\'' || *q == b'"');
    if quote.is_some() {
        i += 1;
    }
    let name_start = i;
    if !src.get(i).is_some_and(|b| ident_start(*b)) {
        return None;
    }
    while i < n && ident_char(src[i]) {
        i += 1;
    }
    let name = &src[name_start..i];
    if let Some(q) = quote {
        if src.get(i) != Some(&q) {
            return None;
        }
        i += 1;
    }
    // The name ends the line.
    if src.get(i) == Some(&b'\r') {
        i += 1;
    }
    if src.get(i) != Some(&b'\n') {
        return None;
    }
    i += 1;
    // The closing name starts a line, after any indenting (PHP 7.3 allows that), and isn't part of a longer name.
    loop {
        if i >= n {
            return Some(n);
        }
        let mut j = i;
        while j < n && (src[j] == b' ' || src[j] == b'\t') {
            j += 1;
        }
        if src[j..].starts_with(name) && !src.get(j + name.len()).is_some_and(|b| ident_char(*b)) {
            return Some(j + name.len());
        }
        match src[i..].iter().position(|b| *b == b'\n') {
            Some(at) => i += at + 1,
            None => return Some(n),
        }
    }
}

/// One change to a file's bytes: what's between `start` and `end` becomes `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

fn edit(range: Range<usize>, text: &str) -> Edit {
    Edit { start: range.start, end: range.end, text: text.to_string() }
}

/// Changes that go together or not at all, such as `array(` and its closing `)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub edits: Vec<Edit>,
    /// What it is, in a few words, for anyone reading.
    pub what: &'static str,
}

impl Change {
    fn one(range: Range<usize>, text: &str, what: &'static str) -> Self {
        Change { edits: vec![edit(range, text)], what }
    }

    /// Whether all of it lies within these bytes, as a change made only in the selection has to.
    pub fn within(&self, range: &Range<usize>) -> bool {
        self.edits.iter().all(|e| e.start >= range.start && e.end <= range.end)
    }
}

/// The file with the changes made. The edits are sorted first, and any that overlap one before are left out.
pub fn apply(src: &[u8], changes: &[Change]) -> Vec<u8> {
    let mut edits: Vec<&Edit> = changes.iter().flat_map(|c| &c.edits).collect();
    edits.sort_by_key(|e| (e.start, e.end));
    let mut out = Vec::with_capacity(src.len() + 64);
    let mut at = 0;
    for e in edits {
        if e.start < at || e.end > src.len() {
            continue;
        }
        out.extend_from_slice(&src[at..e.start]);
        out.extend_from_slice(e.text.as_bytes());
        at = e.end;
    }
    out.extend_from_slice(&src[at..]);
    out
}

/// The tokens that matter for reading code: not spaces or comments.
fn significant(tokens: &[Token]) -> Vec<Token> {
    tokens.iter().copied().filter(|t| !matches!(t.kind, Kind::Space | Kind::Comment)).collect()
}

/// Short tags made long: `<?` becomes `<?php`, and with `echo_too`, `<?=` becomes `<?php echo`. `<?xml` is left
/// as it is, as is `<?=` without `echo_too`, since it works whatever the settings.
pub fn long_tags(src: &[u8], echo_too: bool) -> Vec<Change> {
    let mut out = Vec::new();
    for t in tokens(src) {
        // A space after the new tag, where the old one ran straight on into code (`<?echo`).
        let gap = if src.get(t.end).is_none_or(|b| b.is_ascii_whitespace()) { "" } else { " " };
        match t.kind {
            Kind::Open(Open::Short) => out.push(Change::one(t.start..t.end, &format!("<?php{gap}"), "Short tag")),
            Kind::Open(Open::Echo) if echo_too => out.push(Change::one(t.start..t.end, &format!("<?php echo{gap}"), "Short echo tag")),
            _ => {}
        }
    }
    out
}

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// The causes of "headers already sent" that can be taken away without changing what a page does: a byte order
/// mark or blank lines before the first `<?php`, and a closing `?>` at the end with anything after it (which PHP
/// would send before any header() a file including this one tried).
pub fn headers_fixes(src: &[u8]) -> Vec<Change> {
    let mut out = Vec::new();
    let all = tokens(src);
    let bom = src.starts_with(BOM);
    if bom {
        out.push(Change::one(0..3, "", "Byte order mark"));
    }
    // Blank space before the first opening tag, when that's all there is before it.
    if let [Token { kind: Kind::Html, start, end }, Token { kind: Kind::Open(_), .. }, ..] = all.as_slice() {
        let from = if bom { 3 } else { *start };
        if *end > from && src[from..*end].iter().all(u8::is_ascii_whitespace) {
            out.push(Change::one(from..*end, "", "Blank space before the opening tag"));
        }
    }
    // A closing tag with nothing but blank space after it, to the end.
    let Some(last_close) = all.iter().rposition(|t| t.kind == Kind::Close) else {
        return out;
    };
    let close = all[last_close];
    if close.end == src.len() || !src[close.end..].iter().all(u8::is_ascii_whitespace) {
        return out;
    }
    let eol = if src.windows(2).any(|w| w == b"\r\n") { "\r\n" } else { "\n" };
    // Spaces before the ?> on its line go with it.
    let mut from = close.start;
    while from > 0 && matches!(src[from - 1], b' ' | b'\t') {
        from -= 1;
    }
    let own_line = from == 0 || src[from - 1] == b'\n';
    let mut edits = Vec::new();
    // ?> ends a statement, as a semicolon does; without it, the last statement may need one.
    let code = significant(&all[..last_close]);
    if let Some(last) = code.last()
        && !matches!(last.kind, Kind::Open(_) | Kind::Html | Kind::Close)
        && !(last.kind == Kind::Punct && matches!(last.text(src), b";" | b"}" | b"{" | b":"))
    {
        edits.push(edit(last.end..last.end, ";"));
    }
    edits.push(edit(from..src.len(), if own_line { "" } else { eol }));
    out.push(Change { edits, what: "Closing tag at the end" });
    out
}

/// Whether the token before is one that makes `name(` a method's name, not a call of the function: `->name(`,
/// `::name(`, or `function name(`.
fn is_method_name(code: &[Token], k: usize, src: &[u8]) -> bool {
    let before = |back: usize| k.checked_sub(back).map(|at| code[at]);
    let Some(prev) = before(1) else {
        return false;
    };
    let text = prev.text(src);
    if prev.kind == Kind::Punct && matches!(text, b"->" | b"?->" | b"::") {
        return true;
    }
    if prev.is(src, Kind::Ident, "function") || prev.is(src, Kind::Ident, "const") || prev.is(src, Kind::Ident, "new") {
        return true;
    }
    // function &name(
    prev.kind == Kind::Punct && text == b"&" && before(2).is_some_and(|p| p.is(src, Kind::Ident, "function"))
}

/// The place in `code` of the bracket that closes the one at `open`, counting only code: brackets in strings and
/// comments don't count. None if PHP closes (`?>`) first or the file ends.
fn closing(code: &[Token], open: usize, src: &[u8], pair: (u8, u8)) -> Option<usize> {
    let mut depth = 0usize;
    for (k, t) in code.iter().enumerate().skip(open) {
        match t.kind {
            Kind::Punct if t.text(src) == [pair.0] => depth += 1,
            Kind::Punct if t.text(src) == [pair.1] => {
                depth -= 1;
                if depth == 0 {
                    return Some(k);
                }
            }
            Kind::Close | Kind::Html | Kind::Open(_) => return None,
            _ => {}
        }
    }
    None
}

/// The next token in the whole list after `t` is `(` with nothing but spaces between.
fn paren_follows(all: &[Token], t: &Token, src: &[u8]) -> Option<Token> {
    let at = all.iter().position(|x| x.start == t.end)?;
    all[at..].iter().find(|x| x.kind != Kind::Space).filter(|x| x.kind == Kind::Punct && x.text(src) == b"(").copied()
}

/// `array(...)` written as `[...]`, nested ones too. `(array)` casts, `array` as a type and methods called array
/// are left alone, as is anything in a string, heredoc or comment.
pub fn short_arrays(src: &[u8]) -> Vec<Change> {
    let all = tokens(src);
    let code = significant(&all);
    let mut out = Vec::new();
    for (k, t) in code.iter().enumerate() {
        if !t.is(src, Kind::Ident, "array") || is_method_name(&code, k, src) {
            continue;
        }
        // A comment between array and its bracket would be lost, so that one's left.
        let Some(open) = paren_follows(&all, t, src) else { continue };
        let Some(close) = closing(&code, k + 1, src, (b'(', b')')) else { continue };
        let close = code[close];
        out.push(Change { edits: vec![edit(t.start..open.end, "["), edit(close.start..close.end, "]")], what: "array()" });
    }
    out
}

/// Something PHP 7 or 8 took away, found in a file: where, what, what to use instead, and a fix where one is safe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub start: usize,
    pub end: usize,
    /// What was found, as it's written.
    pub found: String,
    pub instead: Instead,
    /// Which PHP took it away.
    pub removed_in: &'static str,
    pub fix: Option<Change>,
}

/// What replaces something newer PHP took away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instead {
    Mysqli,
    Foreach,
    Closure,
    PregMatch,
    PregMatchCaseless,
    PregReplace,
    PregReplaceCaseless,
    ExplodeOrPregSplit,
    PregSplitCaseless,
    Caseless,
    AlwaysFalse,
    TakeOut,
    CallUserFunc,
    CallUserFuncArray,
    NumberFormatter,
    Hebrev,
    ConvertEncoding,
    IniRestore,
    StripTags,
    Autoload,
    Brackets,
    /// Code to write instead, which isn't translated.
    Code(&'static str),
}

impl Instead {
    /// What it says, in English (translated where it's shown).
    pub fn words(self) -> &'static str {
        match self {
            Instead::Mysqli => "mysqli_* functions or PDO, with prepared statements",
            Instead::Foreach => "A foreach loop",
            Instead::Closure => "An anonymous function: function ($a) { ... }",
            Instead::PregMatch => "preg_match(), with the pattern between slashes",
            Instead::PregMatchCaseless => "preg_match(), with the pattern between slashes and i after",
            Instead::PregReplace => "preg_replace(), with the pattern between slashes",
            Instead::PregReplaceCaseless => "preg_replace(), with the pattern between slashes and i after",
            Instead::ExplodeOrPregSplit => "explode() for plain text, or preg_split() for a pattern",
            Instead::PregSplitCaseless => "preg_split(), with the pattern between slashes and i after",
            Instead::Caseless => "preg_match() with the i modifier",
            Instead::AlwaysFalse => "Nothing: magic quotes are always off, so it's false",
            Instead::TakeOut => "Nothing: take the call out",
            Instead::CallUserFunc => "call_user_func([$object, 'method'], ...)",
            Instead::CallUserFuncArray => "call_user_func_array([$object, 'method'], ...)",
            Instead::NumberFormatter => "NumberFormatter, from the intl extension",
            Instead::Hebrev => "nl2br(hebrev(...))",
            Instead::ConvertEncoding => "mb_convert_encoding() or iconv()",
            Instead::IniRestore => "ini_restore('include_path')",
            Instead::StripTags => "fgets() and strip_tags()",
            Instead::Autoload => "spl_autoload_register()",
            Instead::Brackets => "Square brackets: $s[0]",
            Instead::Code(code) => code,
        }
    }
}

/// Functions taken away, with what replaces each, and which PHP took them.
const GONE: &[(&str, Instead, &str)] = &[
    ("each", Instead::Foreach, "8.0"),
    ("create_function", Instead::Closure, "8.0"),
    ("ereg", Instead::PregMatch, "7.0"),
    ("eregi", Instead::PregMatchCaseless, "7.0"),
    ("ereg_replace", Instead::PregReplace, "7.0"),
    ("eregi_replace", Instead::PregReplaceCaseless, "7.0"),
    ("split", Instead::ExplodeOrPregSplit, "7.0"),
    ("spliti", Instead::PregSplitCaseless, "7.0"),
    ("sql_regcase", Instead::Caseless, "7.0"),
    ("get_magic_quotes_gpc", Instead::AlwaysFalse, "8.0"),
    ("get_magic_quotes_runtime", Instead::AlwaysFalse, "8.0"),
    ("set_magic_quotes_runtime", Instead::TakeOut, "7.0"),
    ("magic_quotes_runtime", Instead::TakeOut, "7.0"),
    ("call_user_method", Instead::CallUserFunc, "7.0"),
    ("call_user_method_array", Instead::CallUserFuncArray, "7.0"),
    ("is_real", Instead::Code("is_float()"), "8.0"),
    ("money_format", Instead::NumberFormatter, "8.0"),
    ("hebrevc", Instead::Hebrev, "8.0"),
    ("convert_cyr_string", Instead::ConvertEncoding, "8.0"),
    ("restore_include_path", Instead::IniRestore, "8.0"),
    ("fgetss", Instead::StripTags, "8.0"),
];

/// The old long names for the request's variables, taken away in PHP 5.4, and the ones that replace them.
const OLD_GLOBALS: &[(&str, &str)] = &[
    ("$HTTP_GET_VARS", "$_GET"),
    ("$HTTP_POST_VARS", "$_POST"),
    ("$HTTP_COOKIE_VARS", "$_COOKIE"),
    ("$HTTP_SERVER_VARS", "$_SERVER"),
    ("$HTTP_ENV_VARS", "$_ENV"),
    ("$HTTP_POST_FILES", "$_FILES"),
    ("$HTTP_SESSION_VARS", "$_SESSION"),
];

/// A string literal's text inside its quotes, if it's a plain one: single quotes, or double quotes with nothing
/// put into it ($ or {).
fn plain_literal(t: &Token, src: &[u8]) -> Option<(u8, String)> {
    let text = std::str::from_utf8(t.text(src)).ok()?;
    let quote = *text.as_bytes().first()?;
    if t.kind != Kind::Str || !matches!(quote, b'\'' | b'"') || text.len() < 2 || !text.ends_with(quote as char) {
        return None;
    }
    let inner = &text[1..text.len() - 1];
    (quote == b'\'' || !inner.contains(['$', '{'])).then(|| (quote, inner.to_string()))
}

/// The old POSIX patterns as PCRE wants them: between slashes, any slash in it escaped.
fn delimited(quote: u8, inner: &str, caseless: bool) -> String {
    let mut out = String::from("/");
    let mut escaped = false;
    for c in inner.chars() {
        if c == '/' && !escaped {
            out.push('\\');
        }
        escaped = c == '\\' && !escaped;
        out.push(c);
    }
    out.push('/');
    if caseless {
        out.push('i');
    }
    format!("{q}{out}{q}", q = quote as char)
}

/// The safe fix for one of the removed functions called at `code[k]`, whose `(` is `code[k + 1]`.
fn safe_fix(code: &[Token], k: usize, src: &[u8], name: &str) -> Option<Change> {
    let call = code[k];
    let close = code[closing(code, k + 1, src, (b'(', b')'))?];
    let first = code.get(k + 2)?;
    match name {
        "get_magic_quotes_gpc" | "get_magic_quotes_runtime" if first.start == close.start => Some(Change::one(call.start..close.end, "false", "Magic quotes")),
        "is_real" => Some(Change::one(call.start..call.end, "is_float", "is_real()")),
        "ereg" | "eregi" | "ereg_replace" | "eregi_replace" | "split" | "spliti" => {
            // Only where the pattern is written out in full, followed by a comma.
            let (quote, inner) = plain_literal(first, src)?;
            if !code.get(k + 3).is_some_and(|c| c.kind == Kind::Punct && c.text(src) == b",") {
                return None;
            }
            let caseless = name.starts_with("eregi") || name == "spliti";
            // split() with plain text, not a pattern, is explode(), which is quicker; with letters, spliti() isn't.
            let plain = !inner.contains(['\\', '.', '[', ']', '(', ')', '*', '+', '?', '{', '}', '|', '^', '$']) && !(caseless && inner.bytes().any(|b| b.is_ascii_alphabetic()));
            let new_name = match name {
                "split" | "spliti" if plain => "explode",
                "split" | "spliti" => "preg_split",
                "ereg" | "eregi" => "preg_match",
                _ => "preg_replace",
            };
            let mut edits = vec![edit(call.start..call.end, new_name)];
            if new_name != "explode" {
                edits.push(edit(first.start..first.end, &delimited(quote, &inner, caseless)));
            }
            Some(Change { edits, what: "Old pattern function" })
        }
        _ => None,
    }
}

/// What PHP 7 and 8 took away that a file still uses, with what replaces each. The safe ones come with their fix:
/// `$s{0}` string offsets, the old $HTTP_*_VARS, magic quotes asked about, is_real(), and ereg() and split() where
/// the pattern is written out. The rest (mysql_*, each(), create_function() and so on) need a person.
pub fn upgrade_findings(src: &[u8]) -> Vec<Finding> {
    let all = tokens(src);
    let code = significant(&all);
    let mut out = Vec::new();
    let found = |t: &Token| String::from_utf8_lossy(t.text(src)).into_owned();
    for (k, t) in code.iter().enumerate() {
        match t.kind {
            Kind::Ident if !is_method_name(&code, k, src) && code.get(k + 1).is_some_and(|p| p.kind == Kind::Punct && p.text(src) == b"(") => {
                let name = String::from_utf8_lossy(t.text(src)).to_ascii_lowercase();
                if name.starts_with("mysql_") {
                    out.push(Finding { start: t.start, end: t.end, found: found(t), instead: Instead::Mysqli, removed_in: "7.0", fix: None });
                } else if let Some((_, instead, removed_in)) = GONE.iter().find(|(gone, ..)| *gone == name) {
                    out.push(Finding { start: t.start, end: t.end, found: found(t), instead: *instead, removed_in, fix: safe_fix(&code, k, src, &name) });
                }
            }
            Kind::Ident if t.is(src, Kind::Ident, "__autoload") && k > 0 && code[k - 1].is(src, Kind::Ident, "function") => {
                out.push(Finding { start: t.start, end: t.end, found: found(t), instead: Instead::Autoload, removed_in: "8.0", fix: None });
            }
            Kind::Var => {
                if let Some((old, new)) = OLD_GLOBALS.iter().find(|(old, _)| t.text(src) == old.as_bytes()) {
                    out.push(Finding { start: t.start, end: t.end, found: old.to_string(), instead: Instead::Code(new), removed_in: "5.4", fix: Some(Change::one(t.start..t.end, new, "Old request variable")) });
                }
            }
            // (real) casts.
            Kind::Ident if t.is(src, Kind::Ident, "real") && k > 0 && code[k - 1].text(src) == b"(" && code.get(k + 1).is_some_and(|c| c.text(src) == b")") => {
                out.push(Finding { start: t.start, end: t.end, found: "(real)".into(), instead: Instead::Code("(float)"), removed_in: "8.0", fix: Some(Change::one(t.start..t.end, "float", "(real) cast")) });
            }
            Kind::Punct if t.text(src) == b"{" => {
                // $s{0}: a string offset in braces, straight after a variable, a property or an index.
                let Some(before) = k.checked_sub(1).map(|b| code[b]) else { continue };
                let after_property = before.kind == Kind::Ident && k >= 2 && matches!(code[k - 2].text(src), b"->" | b"?->");
                let attached = before.end == t.start && (before.kind == Kind::Var || after_property || (before.kind == Kind::Punct && before.text(src) == b"]"));
                if !attached {
                    continue;
                }
                let Some(close) = closing(&code, k, src, (b'{', b'}')) else { continue };
                if close == k + 1 {
                    continue;
                }
                let close = code[close];
                let shown = format!("{}{}", String::from_utf8_lossy(before.text(src)), String::from_utf8_lossy(&src[t.start..close.end]));
                out.push(Finding { start: t.start, end: close.end, found: shown, instead: Instead::Brackets, removed_in: "8.0", fix: Some(Change { edits: vec![edit(t.start..t.end, "["), edit(close.start..close.end, "]")], what: "String offset in braces" }) });
            }
            _ => {}
        }
    }
    out
}

/// A file in an old one-byte encoding, read as UTF-8 would hold it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Legacy {
    pub text: String,
    /// Whether it uses Windows-1252's own characters (curly quotes, the euro and so on), rather than only Latin-1's.
    pub windows: bool,
    /// Some lines with characters beyond ASCII, by number (from 0), to show what they'll look like.
    pub samples: Vec<(usize, String)>,
    /// Lines that still say the page is in the old encoding (a meta tag or a Content-Type header), to change by hand.
    pub declared: Vec<(usize, String)>,
}

/// A file that isn't UTF-8 but reads as Latin-1 or Windows-1252, as UTF-8. None if it's UTF-8 already (or plain
/// ASCII), or looks like something other than text.
pub fn legacy_text(bytes: &[u8]) -> Option<Legacy> {
    if std::str::from_utf8(bytes).is_ok() || bytes.contains(&0) {
        return None;
    }
    let table = crate::charset::known("windows-1252");
    let mut windows = false;
    let text: String = bytes
        .iter()
        .map(|&b| {
            let c = table.decoded[usize::from(b)];
            if (0x80..0xA0).contains(&b) && crate::charset::raw_byte(c).is_none() {
                windows = true;
            }
            // The five bytes Windows-1252 leaves out are Latin-1's control characters.
            if crate::charset::raw_byte(c).is_some() || b < 0x80 { char::from(b) } else { c }
        })
        .collect();
    let mut samples = Vec::new();
    let mut declared = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if samples.len() < 3 && !line.is_ascii() {
            samples.push((number, line.trim().to_string()));
        }
        let lower = line.to_ascii_lowercase();
        if lower.contains("charset") && ["iso-8859-1", "iso8859-1", "latin1", "latin-1", "windows-1252", "cp1252", "iso-8859-15"].iter().any(|name| lower.contains(name)) {
            declared.push((number, line.trim().to_string()));
        }
    }
    Some(Legacy { text, windows, samples, declared })
}

/// Whether F9 should serve the page rather than run it as a script: it has HTML around its PHP, or it reads what a
/// browser sends (`$_GET`, `$_POST` and the like) or sends headers. Anything else, and a file that starts with
/// `#!`, is a script.
pub fn is_web_page(src: &[u8]) -> bool {
    if src.starts_with(b"#!") {
        return false;
    }
    let all = tokens(src);
    let start = usize::from(src.starts_with(BOM)) * 3;
    let html = all.iter().any(|t| t.kind == Kind::Html && src[t.start.max(start)..t.end].iter().any(|b| !b.is_ascii_whitespace()));
    if html {
        return true;
    }
    let web = ["$_GET", "$_POST", "$_REQUEST", "$_COOKIE", "$_SESSION", "$_FILES"];
    let calls = ["header", "session_start", "setcookie", "http_response_code"];
    let code = significant(&all);
    code.iter().enumerate().any(|(k, t)| match t.kind {
        Kind::Var => web.iter().any(|w| t.text(src) == w.as_bytes()),
        Kind::Ident => calls.iter().any(|c| t.is(src, Kind::Ident, c)) && code.get(k + 1).is_some_and(|p| p.text(src) == b"(") && !is_method_name(&code, k, src),
        _ => false,
    })
}

/// A problem `php -l` printed: the line (from 1) and what it said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LintProblem {
    pub line: usize,
    pub message: String,
}

/// The problems in what `php -l` printed, each once (PHP may print one both to its log and to the screen).
pub fn lint_problems(output: &str) -> Vec<LintProblem> {
    let mut out: Vec<LintProblem> = Vec::new();
    for line in output.lines() {
        let line = line.trim().trim_start_matches("PHP ").trim();
        let Some((kind, rest)) = ["Parse error:", "Fatal error:", "Warning:", "Deprecated:", "Notice:"].iter().find_map(|k| line.strip_prefix(k).map(|rest| (k.trim_end_matches(':'), rest))) else {
            continue;
        };
        let Some((said, number)) = rest.rsplit_once(" on line ") else { continue };
        let Ok(number) = number.trim().parse::<usize>() else { continue };
        let said = said.rsplit_once(" in ").map_or(said, |(words, _)| words).trim();
        let message = if kind == "Parse error" { said.to_string() } else { format!("{}: {said}", kind.to_lowercase()) };
        let problem = LintProblem { line: number, message };
        if !out.contains(&problem) {
            out.push(problem);
        }
    }
    out
}

/// The endings of the files the tools look at.
pub const ENDINGS: [&str; 7] = ["php", "phtml", "inc", "php3", "php4", "php5", "phps"];

pub fn is_php_file(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| ENDINGS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// The PHP files in a folder and those in it, leaving out what's not the project's own: Composer's vendor folder,
/// node_modules, and hidden folders such as .git. At most `most` of them.
pub fn php_files_in(folder: &std::path::Path, most: usize) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut waiting = vec![folder.to_path_buf()];
    while let Some(dir) = waiting.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let Ok(kind) = entry.file_type() else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if kind.is_dir() {
                if !name.starts_with('.') && !matches!(name.as_str(), "vendor" | "node_modules") {
                    waiting.push(path);
                }
            } else if kind.is_file() && is_php_file(&path) {
                out.push(path);
                if out.len() >= most {
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

/// One line of a preview: the text there, with what goes struck through and what comes in its place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Same(String),
    Gone(String),
    Added(String),
}

/// A line of a preview: its number (from 0) and its pieces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub line: usize,
    pub pieces: Vec<Piece>,
}

/// Bytes as text to show: UTF-8 as it is, anything else read as Windows-1252; line endings and a byte order
/// mark made visible where they're changed.
fn shown(bytes: &[u8], visible_ends: bool) -> String {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => legacy_text(bytes).map(|l| l.text).unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned()),
    };
    if !visible_ends {
        return text;
    }
    text.replace('\u{FEFF}', "[BOM]").replace("\r\n", "\u{21B5}").replace(['\n', '\r'], "\u{21B5}").replace('\t', "\u{2192}")
}

/// The rows showing what changes make, a row for each line they touch (or several lines, where one change spans
/// them), as Replace in Files previews a replacement.
pub fn preview_rows(src: &[u8], changes: &[Change]) -> Vec<Row> {
    let mut edits: Vec<&Edit> = changes.iter().flat_map(|c| &c.edits).collect();
    edits.sort_by_key(|e| (e.start, e.end));
    let line_of = |at: usize| src[..at.min(src.len())].iter().filter(|b| **b == b'\n').count();
    let line_start = |at: usize| src[..at.min(src.len())].iter().rposition(|b| *b == b'\n').map_or(0, |p| p + 1);
    let line_end = |at: usize| src[at.min(src.len())..].iter().position(|b| *b == b'\n' || *b == b'\r').map_or(src.len(), |p| at + p);
    let mut rows: Vec<Row> = Vec::new();
    // Where the row being built has got to, and where its line ends.
    let mut at = 0;
    let mut row_end = 0;
    for e in edits {
        if e.start < at {
            continue;
        }
        if rows.is_empty() || e.start > row_end {
            if let Some(row) = rows.last_mut() {
                row.pieces.push(Piece::Same(shown(&src[at..row_end.max(at)], false)));
            }
            let start = line_start(e.start);
            rows.push(Row { line: line_of(e.start), pieces: Vec::new() });
            at = start;
        }
        let row = rows.last_mut().expect("one was pushed");
        row.pieces.push(Piece::Same(shown(&src[at..e.start], false)));
        if e.end > e.start {
            row.pieces.push(Piece::Gone(shown(&src[e.start..e.end], true)));
        }
        if !e.text.is_empty() {
            row.pieces.push(Piece::Added(shown(e.text.as_bytes(), true)));
        }
        at = e.end;
        row_end = line_end(e.end);
    }
    if let Some(row) = rows.last_mut() {
        row.pieces.push(Piece::Same(shown(&src[at..row_end.max(at)], false)));
    }
    // Each row starts at its text, its indenting left off, and nothing empty is kept.
    for row in &mut rows {
        if let Some(Piece::Same(first)) = row.pieces.first_mut() {
            *first = first.trim_start().to_string();
        }
        row.pieces.retain(|p| !matches!(p, Piece::Same(s) | Piece::Gone(s) | Piece::Added(s) if s.is_empty()));
    }
    rows
}

/// The line (from 0) a byte is on.
pub fn line_at(src: &[u8], at: usize) -> usize {
    src[..at.min(src.len())].iter().filter(|b| **b == b'\n').count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed(src: &str, changes: &[Change]) -> String {
        String::from_utf8(apply(src.as_bytes(), changes)).unwrap()
    }

    fn tags(src: &str, echo_too: bool) -> String {
        changed(src, &long_tags(src.as_bytes(), echo_too))
    }

    fn arrays(src: &str) -> String {
        changed(src, &short_arrays(src.as_bytes()))
    }

    fn headers(src: &[u8]) -> Vec<u8> {
        apply(src, &headers_fixes(src))
    }

    fn upgraded(src: &str) -> String {
        let fixes: Vec<Change> = upgrade_findings(src.as_bytes()).into_iter().filter_map(|f| f.fix).collect();
        changed(src, &fixes)
    }

    #[test]
    fn the_file_is_read_as_php_reads_it() {
        let src = b"<p><?php echo 'a ?> b'; // c ?>\n<?= $x ?>";
        let kinds: Vec<Kind> = tokens(src).iter().filter(|t| t.kind != Kind::Space).map(|t| t.kind).collect();
        assert_eq!(kinds, [Kind::Html, Kind::Open(Open::Long), Kind::Ident, Kind::Str, Kind::Punct, Kind::Comment, Kind::Close, Kind::Html, Kind::Open(Open::Echo), Kind::Var, Kind::Close], "?> in a string doesn't close PHP, but in a // comment it does");
        let heredoc = b"<?php $a = <<<EOT\n  array( ?> 'x\n  EOT;\n$b = array();";
        let kinds: Vec<Kind> = tokens(heredoc).iter().filter(|t| t.kind != Kind::Space).map(|t| t.kind).collect();
        assert_eq!(kinds, [Kind::Open(Open::Long), Kind::Var, Kind::Punct, Kind::Heredoc, Kind::Punct, Kind::Var, Kind::Punct, Kind::Ident, Kind::Punct, Kind::Punct, Kind::Punct], "an indented closing name ends the heredoc (PHP 7.3)");
        let nowdoc = b"<?php $a = <<<'X'\n\"?>\nX;\n";
        assert!(tokens(nowdoc).iter().any(|t| t.kind == Kind::Heredoc && t.end == nowdoc.len() - 2));
        assert!(!tokens(b"<?php $a << 2;").iter().any(|t| t.kind == Kind::Heredoc), "a shift isn't a heredoc");
    }

    #[test]
    fn short_tags_are_made_long_and_nothing_else() {
        assert_eq!(tags("<? echo 1; ?>", false), "<?php echo 1; ?>");
        assert_eq!(tags("<?echo 1; ?>", false), "<?php echo 1; ?>", "a space where the tag ran into the code");
        assert_eq!(tags("<?\n$a = 1;", false), "<?php\n$a = 1;");
        assert_eq!(tags("<?php echo '<? not a tag ?>'; ?>", false), "<?php echo '<? not a tag ?>'; ?>", "inside a string");
        assert_eq!(tags("<?php /* <? */ // <?\n# <?\n?>", false), "<?php /* <? */ // <?\n# <?\n?>", "inside comments");
        assert_eq!(tags("<?php $s = <<<HTML\n<? x ?>\nHTML;\n$t = <<<'N'\n<?\nN;\n", false), "<?php $s = <<<HTML\n<? x ?>\nHTML;\n$t = <<<'N'\n<?\nN;\n", "inside a heredoc and a nowdoc");
        assert_eq!(tags("<?xml version=\"1.0\"?>\n<? echo 1 ?>", false), "<?xml version=\"1.0\"?>\n<?php echo 1 ?>", "<?xml is left");
        assert_eq!(tags("<?XML version=\"1.0\"?>", false), "<?XML version=\"1.0\"?>");
        assert_eq!(tags("<a href=\"<?=$url?>\" title=\"<? echo $t ?>\">", false), "<a href=\"<?=$url?>\" title=\"<?php echo $t ?>\">", "in an HTML attribute, and <?= left without the option");
        assert_eq!(tags("<a href=\"<?=$url?>\"><?= $a, $b ?>", true), "<a href=\"<?php echo $url?>\"><?php echo $a, $b ?>", "<?= made long when asked");
        assert_eq!(tags("\u{FEFF}<?\r\necho 1;\r\n?>\r\n", false), "\u{FEFF}<?php\r\necho 1;\r\n?>\r\n", "a byte order mark and CRLF are kept");
        assert_eq!(tags("<?php\necho 1;\n", false), "<?php\necho 1;\n", "already long");
    }

    #[test]
    fn headers_already_sent_is_fixed() {
        assert_eq!(headers(b"\xEF\xBB\xBF<?php\nheader('X: y');\n"), b"<?php\nheader('X: y');\n", "the byte order mark goes");
        assert_eq!(headers(b"\n\n  <?php\necho 1;\n"), b"<?php\necho 1;\n", "blank lines before the tag go");
        assert_eq!(headers(b"\xEF\xBB\xBF\r\n<?php\r\necho 1;\r\n"), b"<?php\r\necho 1;\r\n", "both, in a CRLF file");
        assert_eq!(headers(b"<html>\n<?php echo 1; ?>\n</html>\n"), b"<html>\n<?php echo 1; ?>\n</html>\n", "a template is left as it is");
        assert_eq!(headers(b"<?php\nfunction f() {}\n?>\n\n"), b"<?php\nfunction f() {}\n", "a closing tag with blank lines after goes");
        assert_eq!(headers(b"<?php\r\n$a = 1;\r\n?>  \r\n"), b"<?php\r\n$a = 1;\r\n", "in a CRLF file, with spaces after");
        assert_eq!(headers(b"<?php\n$a = 1 ?>\n"), b"<?php\n$a = 1;\n", "?> ended the statement, so a semicolon takes its place");
        assert_eq!(headers(b"<?php\nfoo(); // done ?>\n "), b"<?php\nfoo(); // done\n", "after a comment, the line keeps its ending");
        assert_eq!(headers(b"<?php\nfoo() // done ?>\n "), b"<?php\nfoo(); // done\n", "the semicolon goes before the comment, not in it");
        assert_eq!(headers(b"<?php\necho 1;\n?>"), b"<?php\necho 1;\n?>", "nothing after it: left");
        assert_eq!(headers(b"<?php echo 1; ?>\n<p>Hi</p>\n"), b"<?php echo 1; ?>\n<p>Hi</p>\n", "HTML after it: left");
        assert_eq!(headers(b"<?php\n$s = '?>';\n"), b"<?php\n$s = '?>';\n", "?> in a string isn't a closing tag");
        let fixes = headers_fixes(b"\xEF\xBB\xBF\n<?php\n?>\n");
        assert_eq!(fixes.iter().map(|c| c.what).collect::<Vec<_>>(), ["Byte order mark", "Blank space before the opening tag", "Closing tag at the end"]);
    }

    #[test]
    fn arrays_are_made_short() {
        assert_eq!(arrays("<?php $a = array(1, 2);"), "<?php $a = [1, 2];");
        assert_eq!(arrays("<?php $a = array('x' => array(1, array()), 'y' => f(2));"), "<?php $a = ['x' => [1, []], 'y' => f(2)];", "nested, with other brackets in it");
        assert_eq!(arrays("<?php $a = ARRAY (\n  'a' => 1,\n);"), "<?php $a = [\n  'a' => 1,\n];", "any case, a space before the bracket, over lines");
        assert_eq!(arrays("<?php $a = array('array(' => ')', \"array()\");"), "<?php $a = ['array(' => ')', \"array()\"];", "brackets in strings don't count, and strings aren't changed");
        assert_eq!(arrays("<?php // array(1)\n/* array(2) */ $b = <<<X\narray(3)\nX;\n"), "<?php // array(1)\n/* array(2) */ $b = <<<X\narray(3)\nX;\n", "comments and heredocs are left");
        assert_eq!(arrays("<?php $a = (array) $b; function f(array $x): array { return $x->array(1) + Foo::array(2); }"), "<?php $a = (array) $b; function f(array $x): array { return $x->array(1) + Foo::array(2); }", "casts, types and methods called array are left");
        assert_eq!(arrays("<?php class A { public function array() {} }"), "<?php class A { public function array() {} }");
        assert_eq!(arrays("<?php $a = array(1, /* ) */ 2);"), "<?php $a = [1, /* ) */ 2];", "a bracket in a comment doesn't count");
        assert_eq!(arrays("<?php $a = array(1, 2 ?>"), "<?php $a = array(1, 2 ?>", "not closed before ?>: left");
        assert_eq!(arrays("<p><?php $a = array(1); ?>array(2)</p>"), "<p><?php $a = [1]; ?>array(2)</p>", "text outside PHP is left");
        assert_eq!(arrays("\u{FEFF}<?php\r\n$a = array(\r\n  1,\r\n);\r\n"), "\u{FEFF}<?php\r\n$a = [\r\n  1,\r\n];\r\n");
        // In a selection, a pair goes only if both its brackets are in it.
        let src = "<?php $a = array(1, array(2));";
        let inner = src.find("array(2)").unwrap();
        let picked: Vec<Change> = short_arrays(src.as_bytes()).into_iter().filter(|c| c.within(&(inner..inner + 8))).collect();
        assert_eq!(changed(src, &picked), "<?php $a = array(1, [2]);");
    }

    #[test]
    fn what_newer_php_removed_is_found_and_the_safe_ones_fixed() {
        let src = "<?php\n$r = mysql_query($q);\nwhile (list($k, $v) = each($a)) {}\n$f = create_function('$x', 'return $x;');\n$c = $s{0} . $t->name{$i} . $u['k']{1};\nif (get_magic_quotes_gpc()) {}\n$g = $HTTP_GET_VARS['id'];\n$p = split(',', $line);\n$q = split('[,;]', $line);\nif (eregi('^a/b', $x)) {}\n$y = ereg_replace(\"x+\", 'y', $z);\n$o = $obj->each(1);\nif ($a) {$b = 1;}\n$n = (real) $v;\n";
        let findings = upgrade_findings(src.as_bytes());
        let names: Vec<&str> = findings.iter().map(|f| f.found.as_str()).collect();
        assert_eq!(names, ["mysql_query", "each", "create_function", "$s{0}", "name{$i}", "]{1}", "get_magic_quotes_gpc", "$HTTP_GET_VARS", "split", "split", "eregi", "ereg_replace", "real"].map(|s| if s == "real" { "(real)" } else { s }), "a method called each, and a block after if (), aren't found");
        let fixed = upgraded(src);
        assert!(fixed.contains("mysql_query($q)") && fixed.contains("each($a)") && fixed.contains("create_function("), "those need a person");
        assert!(fixed.contains("$c = $s[0] . $t->name[$i] . $u['k'][1];"), "{}", fixed);
        assert!(fixed.contains("if (false) {}"));
        assert!(fixed.contains("$g = $_GET['id'];"));
        assert!(fixed.contains("$p = explode(',', $line);"), "plain text: explode");
        assert!(fixed.contains("$q = preg_split('/[,;]/', $line);"), "a pattern: preg_split");
        assert!(fixed.contains("if (preg_match('/^a\\/b/i', $x)) {}"), "a slash in it is escaped, and i added: {fixed}");
        assert!(fixed.contains("$y = preg_replace(\"/x+/\", 'y', $z);"));
        assert!(fixed.contains("if ($a) {$b = 1;}"));
        assert!(fixed.contains("$n = (float) $v;"));
        assert!(fixed.contains("$o = $obj->each(1);"));
        let quiet = "<?php\n// mysql_query(1); $s{0}\n$t = 'each($a) $HTTP_GET_VARS';\n$u = \"{$s}\";\n";
        assert!(upgrade_findings(quiet.as_bytes()).is_empty(), "nothing in comments or strings");
        let unsafe_split = "<?php $p = split($pattern, $line); $q = ereg(\"a$b\", $x);";
        assert!(upgrade_findings(unsafe_split.as_bytes()).iter().all(|f| f.fix.is_none()), "a pattern that isn't written out isn't fixed");
    }

    #[test]
    fn old_encodings_are_read_as_utf8() {
        assert_eq!(legacy_text("caf\u{e9}".as_bytes()), None, "already UTF-8");
        assert_eq!(legacy_text(b"plain"), None);
        let latin = legacy_text(b"<?php\necho 'caf\xE9';\n// fine\n<meta charset=\"ISO-8859-1\">\n").unwrap();
        assert_eq!(latin.text, "<?php\necho 'caf\u{e9}';\n// fine\n<meta charset=\"ISO-8859-1\">\n");
        assert!(!latin.windows);
        assert_eq!(latin.samples, [(1, "echo 'caf\u{e9}';".to_string())]);
        assert_eq!(latin.declared.len(), 1);
        let windows = legacy_text(b"\x93quoted\x94 \x80 \x81").unwrap();
        assert_eq!(windows.text, "\u{201C}quoted\u{201D} \u{20AC} \u{81}", "the byte Windows-1252 leaves out is kept as Latin-1 has it");
        assert!(windows.windows);
        assert_eq!(legacy_text(b"a\x00\xE9"), None, "not text");
    }

    #[test]
    fn pages_are_told_from_scripts() {
        assert!(is_web_page(b"<html><?php echo 1; ?></html>"));
        assert!(is_web_page(b"<?php\necho $_GET['a'];\n"));
        assert!(is_web_page(b"<?php\nheader('Location: /');\n"));
        assert!(!is_web_page(b"<?php\necho \"hello\\n\";\n"));
        assert!(!is_web_page(b"#!/usr/bin/env php\n<?php\necho $_GET;\n"));
        assert!(!is_web_page(b"\xEF\xBB\xBF<?php\necho $argv[1];\n"), "a byte order mark isn't HTML");
        assert!(!is_web_page(b"<?php\n$r->header('x');\n"), "a method called header");
    }

    #[test]
    fn what_php_lint_prints_is_read() {
        let printed = "PHP Parse error:  syntax error, unexpected token \"echo\" in /w/bad.php on line 3\n\nParse error: syntax error, unexpected token \"echo\" in /w/bad.php on line 3\nErrors parsing /w/bad.php\n";
        assert_eq!(lint_problems(printed), [LintProblem { line: 3, message: "syntax error, unexpected token \"echo\"".into() }]);
        let fatal = "PHP Fatal error:  Cannot redeclare function f() (previously declared in C:\\w\\dup.php:2) in C:\\w\\dup.php on line 3\n";
        assert_eq!(lint_problems(fatal), [LintProblem { line: 3, message: "fatal error: Cannot redeclare function f() (previously declared in C:\\w\\dup.php:2)".into() }]);
        assert!(lint_problems("No syntax errors detected in ok.php\n").is_empty());
    }

    #[test]
    fn previews_show_each_change_on_its_line() {
        let src = b"<?php\n  $a = array(1,\n    2);\n$b = array();\n";
        let rows = preview_rows(src, &short_arrays(src));
        assert_eq!(rows.iter().map(|r| r.line).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(rows[0].pieces, [Piece::Same("$a = ".into()), Piece::Gone("array(".into()), Piece::Added("[".into()), Piece::Same("1,".into())]);
        assert_eq!(rows[2].pieces, [Piece::Same("$b = ".into()), Piece::Gone("array(".into()), Piece::Added("[".into()), Piece::Gone(")".into()), Piece::Added("]".into()), Piece::Same(";".into())], "two changes on one line, one row");
        let src = b"\xEF\xBB\xBF\r\n<?php\r\necho 1;\r\n?>\r\n\r\n";
        let rows = preview_rows(src, &headers_fixes(src));
        assert_eq!(rows[0].pieces, [Piece::Gone("[BOM]".into()), Piece::Gone("\u{21B5}".into()), Piece::Same("<?php".into())], "what's taken away is shown, line endings too");
        assert_eq!(rows.last().unwrap().pieces, [Piece::Gone("?>\u{21B5}\u{21B5}".into())]);
    }

    #[test]
    fn a_folder_s_php_files_are_found_but_not_vendor_s() {
        let dir = std::env::temp_dir().join(format!("limoniter-php-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for file in ["index.php", "lib/a.inc", "lib/b.PHTML", "vendor/x/y.php", ".git/z.php", "readme.txt"] {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "<?php\n").unwrap();
        }
        let found: Vec<String> = php_files_in(&dir, 100).iter().map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(found, ["index.php", "lib/a.inc", "lib/b.PHTML"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
