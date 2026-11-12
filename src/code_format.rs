//! Laying out code that uses braces: JavaScript and the C family. Only the space between pieces of code changes,
//! and what's in strings and comments stays exactly as it is.

/// What differs between the languages.
#[derive(Clone, Copy, Debug)]
pub struct Rules {
    /// `/` can start a pattern, and backticks start a string.
    pub script: bool,
    /// `#` at the start of a line is for the preprocessor.
    pub preprocessor: bool,
    /// Braces may move to or from their own line. In JavaScript a line break can change what code means, so they don't.
    pub moves_braces: bool,
    /// With `moves_braces`, an opening brace goes on its own line.
    pub break_braces: bool,
}

pub const SCRIPT: Rules = Rules { script: true, preprocessor: false, moves_braces: false, break_braces: false };
pub const C_ATTACH: Rules = Rules { script: false, preprocessor: true, moves_braces: true, break_braces: false };
pub const C_BREAK: Rules = Rules { script: false, preprocessor: true, moves_braces: true, break_braces: true };

const UNBALANCED: &str = "The brackets in this don't balance, so nothing was changed. If it's part of a file, select the whole of it.";
const MOST_INLINE: usize = 60;
const MOST_LOOKAHEAD: usize = 2000;

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Word(String),
    Punct(char),
    Str(String),
    LineComment(String),
    BlockComment(String),
    Preprocessor(String),
    /// Space between pieces, and how many line breaks are in it.
    Space(usize),
}

fn regex_can_follow(previous: Option<&Tok>) -> bool {
    match previous {
        None => true,
        Some(Tok::Punct(c)) => matches!(c, '(' | ',' | '[' | '{' | '}' | ';'),
        Some(Tok::Word(word)) => {
            matches!(word.as_str(), "return" | "typeof" | "in" | "of" | "instanceof" | "void" | "delete" | "case" | "new" | "yield" | "await") || word.ends_with(['=', '+', '-', '*', '%', '<', '>', '~', '^', '!', '&', '|', '?', ':'])
        }
        _ => false,
    }
}

/// How far a backtick string runs, from just after its opening backtick. Code in `${}` can hold more of them.
fn template_end(chars: &[char], mut at: usize) -> Option<usize> {
    while at < chars.len() {
        match chars[at] {
            '\\' => at += 2,
            '`' => return Some(at + 1),
            '$' if chars.get(at + 1) == Some(&'{') => {
                at += 2;
                let mut depth = 1;
                while depth > 0 {
                    match *chars.get(at)? {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        '`' => at = template_end(chars, at + 1)? - 1,
                        _ => {}
                    }
                    at += 1;
                }
            }
            _ => at += 1,
        }
    }
    None
}

fn lex(text: &str, rules: &Rules) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().collect();
    let mut toks: Vec<Tok> = Vec::new();
    let mut word = String::new();
    let mut at = 0;
    let mut line_start = true;
    let piece = |from: usize, to: usize| chars[from..to.min(chars.len())].iter().collect::<String>();
    while at < chars.len() {
        let c = chars[at];
        let next = chars.get(at + 1).copied();
        let special = c.is_whitespace() || matches!(c, '{' | '}' | '(' | ')' | '[' | ']' | ';' | ',' | '"' | '\'' | '/') || (c == '`' && rules.script) || (c == '#' && rules.preprocessor && line_start);
        if special && !word.is_empty() {
            toks.push(Tok::Word(std::mem::take(&mut word)));
        }
        if c.is_whitespace() {
            let mut breaks = 0;
            while at < chars.len() && chars[at].is_whitespace() {
                breaks += usize::from(chars[at] == '\n');
                at += 1;
            }
            line_start |= breaks > 0;
            toks.push(Tok::Space(breaks));
            continue;
        }
        if c == '#' && rules.preprocessor && line_start {
            let from = at;
            while at < chars.len() && !(chars[at] == '\n' && chars[at - 1] != '\\') {
                at += 1;
            }
            toks.push(Tok::Preprocessor(piece(from, at).trim_end().to_string()));
            continue;
        }
        line_start = false;
        if c == '/' && next == Some('/') {
            let from = at;
            while at < chars.len() && chars[at] != '\n' {
                at += 1;
            }
            toks.push(Tok::LineComment(piece(from, at).trim_end().to_string()));
            continue;
        }
        if c == '/' && next == Some('*') {
            let from = at;
            at += 2;
            while at < chars.len() && !(chars[at] == '*' && chars.get(at + 1) == Some(&'/')) {
                at += 1;
            }
            at = (at + 2).min(chars.len());
            toks.push(Tok::BlockComment(piece(from, at)));
            continue;
        }
        if c == '/' && rules.script && regex_can_follow(toks.iter().rev().find(|tok| !matches!(tok, Tok::Space(_)))) {
            let (mut end, mut in_class) = (at + 1, false);
            let mut closed = false;
            while end < chars.len() && chars[end] != '\n' {
                match chars[end] {
                    '\\' => end += 1,
                    '[' => in_class = true,
                    ']' => in_class = false,
                    '/' if !in_class => {
                        closed = true;
                        break;
                    }
                    _ => {}
                }
                end += 1;
            }
            if closed {
                toks.push(Tok::Str(piece(at, end + 1)));
                at = end + 1;
                continue;
            }
        }
        if c == '`' && rules.script {
            let end = template_end(&chars, at + 1).unwrap_or(chars.len());
            toks.push(Tok::Str(piece(at, end)));
            at = end;
            continue;
        }
        if c == '"' || c == '\'' {
            let from = at;
            at += 1;
            while at < chars.len() && chars[at] != c {
                if chars[at] == '\n' {
                    break;
                }
                if chars[at] == '\\' {
                    at += 1;
                }
                at += 1;
            }
            at = (at + usize::from(chars.get(at) == Some(&c))).min(chars.len());
            toks.push(Tok::Str(piece(from, at)));
            continue;
        }
        if matches!(c, '{' | '}' | '(' | ')' | '[' | ']' | ';' | ',') {
            toks.push(Tok::Punct(c));
        } else {
            word.push(c);
        }
        at += 1;
    }
    if !word.is_empty() {
        toks.push(Tok::Word(word));
    }
    toks
}

struct Out<'a> {
    lines: Vec<String>,
    cur: String,
    indent: usize,
    unit: &'a str,
}

impl Out<'_> {
    fn push(&mut self, text: &str, level: usize) {
        if self.cur.is_empty() {
            self.indent = level;
        }
        self.cur.push_str(text);
    }

    fn flush(&mut self) {
        let kept = self.cur.trim_end_matches([' ', '\t']).len();
        self.cur.truncate(kept);
        if !self.cur.is_empty() {
            self.lines.push(format!("{}{}", self.unit.repeat(self.indent), self.cur));
            self.cur.clear();
        }
    }
}

fn next_solid(toks: &[Tok], mut at: usize) -> Option<&Tok> {
    while let Some(tok) = toks.get(at) {
        if !matches!(tok, Tok::Space(_)) {
            return Some(tok);
        }
        at += 1;
    }
    None
}

/// Whether what follows, on the same line, is a comment that belongs on this line.
fn comment_follows(toks: &[Tok], at: usize) -> bool {
    match toks.get(at) {
        Some(Tok::LineComment(_)) => true,
        Some(Tok::Space(0)) => matches!(toks.get(at + 1), Some(Tok::LineComment(_))),
        _ => false,
    }
}

/// Whether a brace's contents stay on one line, as with `x = {1, 2}`. Returns the token after the closing brace.
fn inline_group(toks: &[Tok], open: usize, previous: Option<&Tok>) -> Option<usize> {
    let empty = matches!(next_solid(toks, open + 1), Some(Tok::Punct('}')));
    let value = match previous {
        Some(Tok::Punct(c)) => matches!(c, '(' | ',' | '['),
        Some(Tok::Word(word)) => word == "return" || word.ends_with(['=', ':']),
        _ => false,
    };
    if !empty && !value {
        return None;
    }
    let (mut depth, mut size, mut at) = (0, 0, open);
    while let Some(tok) = toks.get(at) {
        match tok {
            Tok::Punct('{') => depth += 1,
            Tok::Punct('}') => depth -= 1,
            Tok::Punct(';') | Tok::LineComment(_) | Tok::BlockComment(_) | Tok::Preprocessor(_) => return None,
            Tok::Space(breaks) if *breaks > 0 && !empty => return None,
            Tok::Word(text) | Tok::Str(text) => size += text.chars().count(),
            _ => size += 1,
        }
        if size > MOST_INLINE {
            return None;
        }
        at += 1;
        if depth == 0 {
            return Some(at);
        }
    }
    None
}

/// Whether a round or square bracket's contents count as a level of indent. Not if they hold a block,
/// because the block is indented already.
fn bracket_counts(toks: &[Tok], open: usize) -> bool {
    let mut depth = 0;
    for tok in toks.iter().skip(open).take(MOST_LOOKAHEAD) {
        match tok {
            Tok::Punct('(' | '[') => depth += 1,
            Tok::Punct(')' | ']') => {
                depth -= 1;
                if depth == 0 {
                    return true;
                }
            }
            Tok::Punct('{') => return false,
            _ => {}
        }
    }
    true
}

fn word_of(tok: Option<&Tok>) -> Option<&str> {
    match tok {
        Some(Tok::Word(word)) => Some(word),
        _ => None,
    }
}

/// Lays code out by its braces. Existing line breaks stay. Breaks are added after `{` and `;`, around `}`, and after a
/// comma directly inside braces, none of which change what the code means.
pub fn format(text: &str, unit: &str, rules: &Rules) -> Result<String, String> {
    let toks = lex(text, rules);
    let mut out = Out { lines: Vec::new(), cur: String::new(), indent: 0, unit };
    // Each open bracket, and whether it counts as a level of indent.
    let mut stack: Vec<(char, bool)> = Vec::new();
    let level = |stack: &[(char, bool)]| stack.iter().filter(|(_, counts)| *counts).count();
    let mut previous: Option<Tok> = None;
    let mut at = 0;
    while at < toks.len() {
        let tok = &toks[at];
        match tok {
            Tok::Space(0) => {
                if !out.cur.is_empty() && !out.cur.ends_with(' ') {
                    out.cur.push(' ');
                }
            }
            Tok::Space(breaks) => {
                out.flush();
                if *breaks > 1 && out.lines.last().is_some_and(|last| !last.is_empty()) {
                    out.lines.push(String::new());
                }
            }
            Tok::Preprocessor(raw) => {
                out.flush();
                out.lines.push(raw.clone());
            }
            Tok::LineComment(raw) => {
                out.push(raw, level(&stack));
                out.flush();
            }
            Tok::BlockComment(raw) | Tok::Str(raw) => out.push(raw, level(&stack)),
            Tok::Word(word) => out.push(word, level(&stack)),
            Tok::Punct('{') => {
                if let Some(end) = inline_group(&toks, at, previous.as_ref()) {
                    if matches!(previous, Some(Tok::Punct(')'))) && !out.cur.is_empty() && !out.cur.ends_with(' ') {
                        out.cur.push(' ');
                    }
                    for inner in &toks[at..end] {
                        match inner {
                            Tok::Word(text) | Tok::Str(text) => out.push(text, level(&stack)),
                            Tok::Punct(c) => out.push(c.encode_utf8(&mut [0; 4]), level(&stack)),
                            _ if out.cur.ends_with(' ') => {}
                            _ => out.cur.push(' '),
                        }
                    }
                    previous = Some(Tok::Punct('}'));
                    at = end;
                    continue;
                }
                let is_block = !matches!(&previous, Some(Tok::Word(word)) if word.ends_with('='));
                if rules.moves_braces && is_block && rules.break_braces {
                    out.flush();
                } else if rules.moves_braces && out.cur.is_empty() {
                    // Up on to the end of the line before, unless that would put it inside a comment or a directive.
                    let can_join = matches!(previous, Some(Tok::Word(_) | Tok::Punct(')' | ']') | Tok::Str(_)));
                    if can_join && let Some(last) = out.lines.pop_if(|last| !last.is_empty()) {
                        let spaces = last.len() - last.trim_start_matches(unit).len();
                        out.indent = if unit.is_empty() { 0 } else { spaces / unit.len() };
                        out.cur = last[spaces..].to_string();
                    }
                }
                if !out.cur.is_empty() && !out.cur.ends_with([' ', '(', '[']) {
                    out.cur.push(' ');
                }
                out.push("{", level(&stack));
                stack.push(('{', true));
                if !comment_follows(&toks, at + 1) {
                    out.flush();
                }
            }
            Tok::Punct('}') => {
                out.flush();
                if stack.pop().map(|(c, _)| c) != Some('{') {
                    return Err(UNBALANCED.to_string());
                }
                out.push("}", level(&stack));
                let same_line = match toks.get(at + 1) {
                    Some(Tok::Space(0)) => toks.get(at + 2),
                    other => other,
                };
                let following = word_of(next_solid(&toks, at + 1));
                let continues = matches!(following, Some("else" | "catch" | "finally"));
                if rules.moves_braces && continues {
                    out.flush();
                    if !rules.break_braces {
                        // `} else` stays together, on whichever lines it was written.
                        let last = out.lines.pop().unwrap_or_default();
                        out.cur = last.trim_start_matches(unit).to_string();
                        out.cur.push(' ');
                        out.indent = level(&stack);
                        while matches!(toks.get(at + 1), Some(Tok::Space(_))) {
                            at += 1;
                        }
                    }
                } else {
                    match same_line {
                        Some(Tok::Word(word)) if word.starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '$') && !continues && !matches!(word.as_str(), "while" | "else" | "catch" | "finally" | "from" | "as") => out.flush(),
                        None | Some(Tok::Space(_) | Tok::Preprocessor(_)) => out.flush(),
                        _ => {}
                    }
                }
            }
            Tok::Punct(c @ ('(' | '[')) => {
                out.push(c.encode_utf8(&mut [0; 4]), level(&stack));
                stack.push((*c, bracket_counts(&toks, at)));
            }
            Tok::Punct(c @ (')' | ']')) => {
                let wanted = if *c == ')' { '(' } else { '[' };
                if stack.pop().map(|(open, _)| open) != Some(wanted) {
                    return Err(UNBALANCED.to_string());
                }
                out.push(c.encode_utf8(&mut [0; 4]), level(&stack));
            }
            Tok::Punct(';') => {
                out.push(";", level(&stack));
                if !matches!(stack.last(), Some(('(' | '[', _))) && !comment_follows(&toks, at + 1) {
                    out.flush();
                }
            }
            Tok::Punct(',') => {
                out.push(",", level(&stack));
                if matches!(stack.last(), Some(('{', _))) && !comment_follows(&toks, at + 1) {
                    out.flush();
                }
            }
            Tok::Punct(other) => out.push(other.encode_utf8(&mut [0; 4]), level(&stack)),
        }
        if !matches!(tok, Tok::Space(_)) {
            previous = Some(tok.clone());
        }
        at += 1;
    }
    out.flush();
    if !stack.is_empty() {
        return Err(UNBALANCED.to_string());
    }
    while out.lines.last().is_some_and(String::is_empty) {
        out.lines.pop();
    }
    Ok(out.lines.join("\n"))
}

/// Takes out comments, indents and blank lines, keeping every line break that's left, because in JavaScript taking
/// one away can change what the code means.
pub fn compress(text: &str, rules: &Rules) -> String {
    let mut out = String::with_capacity(text.len());
    for tok in lex(text, rules) {
        match tok {
            Tok::Word(raw) | Tok::Str(raw) | Tok::Preprocessor(raw) => out.push_str(&raw),
            Tok::Punct(c) => out.push(c),
            Tok::LineComment(_) => {}
            Tok::BlockComment(raw) => {
                let ending = if raw.contains('\n') { '\n' } else { ' ' };
                if !out.is_empty() && !out.ends_with(['\n', ' ']) {
                    out.push(ending);
                }
            }
            Tok::Space(breaks) => {
                if breaks > 0 {
                    let kept = out.trim_end_matches(' ').len();
                    out.truncate(kept);
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                } else if !out.is_empty() && !out.ends_with(['\n', ' ']) {
                    out.push(' ');
                }
            }
        }
    }
    let kept = out.trim_end().len();
    out.truncate(kept);
    out
}

/// The code without its comments and spaces, to check nothing else has changed.
pub fn solid_code(text: &str, rules: &Rules) -> String {
    let mut out = String::with_capacity(text.len());
    for tok in lex(text, rules) {
        match tok {
            Tok::Word(raw) | Tok::Str(raw) | Tok::Preprocessor(raw) => out.push_str(&raw),
            Tok::Punct(c) => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn js(text: &str) -> String {
        format(text, "  ", &SCRIPT).unwrap()
    }

    fn c(text: &str) -> String {
        format(text, "  ", &C_ATTACH).unwrap()
    }

    #[test]
    fn minified_script_opens_out() {
        assert_eq!(js("function a(b){if(b){return 1;}else{return 2;}}"), "function a(b) {\n  if(b) {\n    return 1;\n  }else {\n    return 2;\n  }\n}");
    }

    #[test]
    fn semicolons_in_for_loops_dont_break_lines() {
        assert_eq!(js("for(var i=0;i<3;i++){a(i);}"), "for(var i=0;i<3;i++) {\n  a(i);\n}");
    }

    #[test]
    fn line_breaks_already_there_stay_so_meaning_cant_change() {
        assert_eq!(js("a = 1\nb = 2"), "a = 1\nb = 2");
        assert_eq!(js("return\n{ x }"), "return\n{ x }", "a brace is never pulled up after return");
        assert_eq!(js("x = {\na: 1}"), "x = {\n  a: 1\n}");
    }

    #[test]
    fn strings_comments_and_patterns_are_left_alone() {
        let text = "a = \"{ ; }\";\nb = '{';\nc = `{ ${d + `}`} ;\n   }`;\ne = /[/{;]+\\//g;\n// { ;\n/* { ; */\nf = g / 2 / h;";
        assert_eq!(js(text), text);
    }

    #[test]
    fn objects_break_at_commas_unless_short_values() {
        assert_eq!(js("a({b:1,c:2});"), "a({b:1,c:2});", "short values stay on the line");
        assert_eq!(js("function f(){return {a:1};}"), "function f() {\n  return {a:1};\n}");
        let long = format!("x = {{a:\"{0}\",b:\"{0}\"}};", "y".repeat(50));
        assert_eq!(js(&long).lines().count(), 4);
    }

    #[test]
    fn empty_blocks_stay_together_and_chains_stay_attached() {
        assert_eq!(js("function f(){}\nx();"), "function f() {}\nx();");
        assert_eq!(js("(function(){a();})();"), "(function() {\n  a();\n})();");
        assert_eq!(js("a(function(){b();},1);"), "a(function() {\n  b();\n},1);");
    }

    #[test]
    fn trailing_comments_stay_on_their_lines() {
        assert_eq!(js("a(); // why\nif (b) { // because\nc();\n}"), "a(); // why\nif (b) { // because\n  c();\n}");
    }

    #[test]
    fn blank_lines_shrink_to_one_and_indent_is_replaced() {
        assert_eq!(js("      a;\n\n\n\n      b;"), "a;\n\nb;");
    }

    #[test]
    fn reformatting_twice_gives_the_same_result() {
        for text in ["function a(b){if(b){return 1;}else{return [1,2];}}", "x = {a:1,\nb:[1,2,{c:3}]};\n\ny(function(){z();});"] {
            let once = js(text);
            assert_eq!(js(&once), once);
        }
    }

    #[test]
    fn unbalanced_brackets_change_nothing() {
        for text in ["a {", "}", "(]", "f(;", "{ ( }"] {
            assert!(format(text, " ", &SCRIPT).is_err(), "{text}");
        }
    }

    #[test]
    fn nothing_but_space_ever_changes() {
        let samples = ["function a(b,c){var d={e:1,f:[1,2,3]};for(;;){g(`h${i}`,/j/);}return d;}", "if(a){b}else if(c){d}else{e}", "switch(a){case 1:b();break;default:c();}", "class A extends B{constructor(){super();}}", "x=>{y};z=(a)=>({b:1});"];
        for text in samples {
            let done = js(text);
            let squeeze = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
            assert_eq!(squeeze(&done), squeeze(text));
        }
    }

    #[test]
    fn c_braces_attach_or_break_as_asked() {
        let text = "void f()\n{\nif (a)\n{\nb();\n}\nelse\n{\nc();\n}\n}";
        assert_eq!(c(text), "void f() {\n  if (a) {\n    b();\n  } else {\n    c();\n  }\n}");
        let open = "void f() {\nif (a) {\nb();\n} else {\nc();\n}\n}";
        assert_eq!(format(open, "  ", &C_BREAK).unwrap(), "void f()\n{\n  if (a)\n  {\n    b();\n  }\n  else\n  {\n    c();\n  }\n}");
    }

    #[test]
    fn c_preprocessor_lines_stay_where_they_are_and_dont_count() {
        assert_eq!(c("#include <a>\n#define X { \\\n  1 }\nint y;"), "#include <a>\n#define X { \\\n  1 }\nint y;");
        assert_eq!(c("void f() {\n#ifdef A\nb();\n#endif\n}"), "void f() {\n#ifdef A\n  b();\n#endif\n}");
    }

    #[test]
    fn a_brace_isnt_pulled_up_on_to_a_comment() {
        assert_eq!(c("void f() // note\n{\nb();\n}"), "void f() // note\n{\n  b();\n}");
    }

    #[test]
    fn c_initialisers_stay_put_with_either_style() {
        assert_eq!(format("int a[] = {1, 2, 3};", "  ", &C_BREAK).unwrap(), "int a[] = {1, 2, 3};");
    }

    #[test]
    fn compress_takes_out_comments_indents_and_blanks_and_keeps_line_breaks() {
        let text = "// top\n  a = 1; // one\n\n  b = /* two */ \"// not a comment\";\n";
        assert_eq!(compress(text, &SCRIPT), "a = 1;\nb = \"// not a comment\";");
        assert_eq!(solid_code(&compress(text, &SCRIPT), &SCRIPT), solid_code(text, &SCRIPT));
    }

    #[test]
    fn compress_doesnt_join_what_a_comment_kept_apart() {
        assert_eq!(compress("a/* x */b", &SCRIPT), "a b");
        assert_eq!(compress("a\n/* x\ny */\nb", &SCRIPT), "a\nb");
    }

    #[test]
    fn odd_input_never_goes_wrong() {
        for text in ["", "\"", "'", "`", "/", "/*", "//", "${", "`${", "`${`", "#", "\\", "{}", "()", "a\\", "/[", "/a\\"] {
            let _ = format(text, " ", &SCRIPT);
            let _ = format(text, " ", &C_ATTACH);
            let _ = compress(text, &SCRIPT);
        }
    }
}
