//! Doc comments from a function's signature: each parameter and what it returns listed, in the language's own
//! way (Python's docstrings, JSDoc and Doxygen blocks, C#'s XML comments, Rust's ///, Go's comment naming the
//! function), as autoDocstring and the Doxygen generator write them.

/// How a language writes its doc comments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Python,
    /// /** ... */ with @param and @return: C, C++, Java, JavaScript, TypeScript, PHP, Kotlin.
    Block,
    CSharp,
    Rust,
    Swift,
    Go,
}

pub fn style_for(language: &str) -> Option<Style> {
    Some(match language {
        "Python" => Style::Python,
        "C" | "C++" | "Objective-C" | "Objective-C++" | "Java" | "JavaScript" | "JavaScript (Babel)" | "TypeScript" | "TypeScriptReact" | "PHP" | "Kotlin" | "Scala" | "Dart" | "Groovy" => Style::Block,
        "C#" => Style::CSharp,
        "Rust" => Style::Rust,
        "Swift" => Style::Swift,
        "Go" => Style::Go,
        _ => return None,
    })
}

/// A function's signature as read: its name, its parameters' names, and whether it returns anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub name: String,
    pub params: Vec<String>,
    pub returns: bool,
}

/// Words before a C-like function's name that aren't its return type.
const MODIFIERS: [&str; 20] = ["public", "private", "protected", "static", "inline", "extern", "virtual", "final", "abstract", "async", "export", "default", "override", "const", "unsafe", "pub", "function", "fn", "func", "fun"];

/// Splits at the commas not inside brackets, quotes or generics.
fn split_top(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let (mut depth, mut current, mut quote) = (0i32, String::new(), None);
    for c in text.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[' | '{' | '<') => depth += 1,
            (None, ')' | ']' | '}' | '>') => depth -= 1,
            (None, ',') if depth == 0 => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts.into_iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

/// A parameter's name, from how the language writes one.
fn param_name(param: &str, style: Style) -> Option<String> {
    let param = param.split('=').next()?.trim();
    let name = match style {
        // name: Type, or *args, or self.
        Style::Python | Style::Rust | Style::Swift => {
            let before = param.split(':').next()?.trim();
            // Swift's outer label, Rust's mut and patterns: the last word before the colon.
            before.split_whitespace().last()?.trim_start_matches(['*', '&']).to_string()
        }
        // name Type
        Style::Go => param.split_whitespace().next()?.to_string(),
        Style::Block | Style::CSharp => {
            // TypeScript and Kotlin have name: Type; C, Java, C# and PHP have Type name.
            if let Some((before, _)) = param.split_once(':')
                && !before.contains(' ')
            {
                before.trim_end_matches('?').to_string()
            } else {
                let words: Vec<&str> = param.split_whitespace().collect();
                words.last()?.trim_start_matches(['*', '&', '$']).trim_end_matches("[]").to_string()
            }
        }
    };
    let name = name.trim_start_matches(['*', '&', '$', '.']).to_string();
    (!name.is_empty() && !matches!(name.as_str(), "self" | "cls" | "this" | "void") && name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')).then_some(name)
}

/// Reads a signature from its lines (it may run over several), if they hold one.
pub fn signature(text: &str, style: Style) -> Option<Signature> {
    let chars: Vec<char> = text.chars().collect();
    // A Go method's receiver comes first, in brackets of its own: the parameters are the next ones.
    let skip = if style == Style::Go && text.trim_start().starts_with("func (") { chars.iter().position(|c| *c == ')')? + 1 } else { 0 };
    let open = skip + chars[skip..].iter().position(|c| *c == '(')?;
    // Its closing bracket.
    let mut depth = 0;
    let mut close = None;
    for (i, c) in chars.iter().enumerate().skip(open) {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let before: String = chars[skip..open].iter().collect();
    let words: Vec<&str> = before.split(|c: char| c.is_whitespace()).filter(|w| !w.is_empty()).collect();
    let name = words.last()?.trim_start_matches(['*', '&']).split(['<', '.']).next_back()?.to_string();
    if name.is_empty() || !name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') || matches!(name.as_str(), "if" | "for" | "while" | "switch" | "return" | "catch") {
        return None;
    }
    let after: String = chars[close + 1..].iter().collect();
    let after = after.trim();
    let params = split_top(&chars[open + 1..close].iter().collect::<String>()).iter().filter_map(|p| param_name(p, style)).collect();
    let returns = match style {
        Style::Python => after.starts_with("->") && !after.starts_with("-> None"),
        Style::Rust | Style::Swift => after.starts_with("->") && !after.starts_with("-> ()") && !after.starts_with("-> Void"),
        Style::Go => !after.is_empty() && !after.starts_with('{'),
        Style::Block | Style::CSharp => {
            if let Some(kind) = after.strip_prefix(':') {
                // TypeScript's and Kotlin's return types.
                !kind.trim_start().starts_with("void") && !kind.trim_start().starts_with("Unit")
            } else {
                let kinds: Vec<&&str> = words[..words.len() - 1].iter().filter(|w| !MODIFIERS.contains(&w.to_lowercase().as_str())).collect();
                !kinds.is_empty() && !kinds.iter().any(|w| **w == "void" || w.ends_with("void"))
            }
        }
    };
    Some(Signature { name, params, returns })
}

/// The comment's lines, each with the indent given, and which of them the cursor goes to (the summary's).
pub fn comment(sig: &Signature, style: Style, indent: &str) -> (Vec<String>, usize) {
    let mut out: Vec<String> = Vec::new();
    let summary;
    match style {
        Style::Python => {
            out.push(format!("{indent}\"\"\""));
            summary = 0;
            if !sig.params.is_empty() || sig.returns {
                out.push(String::new());
            }
            if !sig.params.is_empty() {
                out.push(format!("{indent}Args:"));
                out.extend(sig.params.iter().map(|p| format!("{indent}    {p}: ")));
            }
            if sig.returns {
                if !sig.params.is_empty() {
                    out.push(String::new());
                }
                out.push(format!("{indent}Returns:"));
                out.push(format!("{indent}    "));
            }
            out.push(format!("{indent}\"\"\""));
        }
        Style::Block => {
            out.push(format!("{indent}/**"));
            out.push(format!("{indent} * "));
            summary = 1;
            if !sig.params.is_empty() || sig.returns {
                out.push(format!("{indent} *"));
            }
            out.extend(sig.params.iter().map(|p| format!("{indent} * @param {p} ")));
            if sig.returns {
                out.push(format!("{indent} * @return "));
            }
            out.push(format!("{indent} */"));
        }
        Style::CSharp => {
            out.push(format!("{indent}/// <summary>"));
            out.push(format!("{indent}/// "));
            summary = 1;
            out.push(format!("{indent}/// </summary>"));
            out.extend(sig.params.iter().map(|p| format!("{indent}/// <param name=\"{p}\"></param>")));
            if sig.returns {
                out.push(format!("{indent}/// <returns></returns>"));
            }
        }
        Style::Rust | Style::Swift => {
            out.push(format!("{indent}/// "));
            summary = 0;
            if !sig.params.is_empty() {
                out.push(format!("{indent}///"));
                if style == Style::Rust {
                    out.push(format!("{indent}/// # Arguments"));
                    out.push(format!("{indent}///"));
                    out.extend(sig.params.iter().map(|p| format!("{indent}/// * `{p}` - ")));
                } else {
                    out.push(format!("{indent}/// - Parameters:"));
                    out.extend(sig.params.iter().map(|p| format!("{indent}///   - {p}: ")));
                }
            }
            if sig.returns {
                out.push(format!("{indent}///"));
                out.push(if style == Style::Rust { format!("{indent}/// Returns ") } else { format!("{indent}/// - Returns: ") });
            }
        }
        Style::Go => {
            out.push(format!("{indent}// {} ", sig.name));
            summary = 0;
        }
    }
    (out, summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_are_read_in_each_language() {
        let py = signature("def mix(self, a: int, b=2, *rest, **kw) -> float:", Style::Python).unwrap();
        assert_eq!((py.name.as_str(), py.params.clone(), py.returns), ("mix", vec!["a".into(), "b".into(), "rest".into(), "kw".into()], true));
        let c = signature("static unsigned char *copy_line(char *dest, const char src[], int n)", Style::Block).unwrap();
        assert_eq!((c.name.as_str(), c.params.clone(), c.returns), ("copy_line", vec!["dest".into(), "src".into(), "n".into()], true));
        assert!(!signature("void draw(Sprite s) {", Style::Block).unwrap().returns);
        let ts = signature("export async function load(url: string, retries?: number): Promise<Data> {", Style::Block).unwrap();
        assert_eq!((ts.params.clone(), ts.returns), (vec!["url".into(), "retries".into()], true));
        let rust = signature("pub fn scale(&self, by: f32, mut out: Vec<u8>) -> Vec<u8> {", Style::Rust).unwrap();
        assert_eq!((rust.params.clone(), rust.returns), (vec!["by".into(), "out".into()], true));
        let go = signature("func (s *Server) Serve(addr string, h Handler) error {", Style::Go).unwrap();
        assert_eq!((go.name.as_str(), go.params.clone(), go.returns), ("Serve", vec!["addr".into(), "h".into()], true));
        assert_eq!(signature("if (x > 1) {", Style::Block), None);
    }

    #[test]
    fn comments_are_written_in_each_languages_way() {
        let sig = Signature { name: "mix".into(), params: vec!["a".into(), "b".into()], returns: true };
        let (lines, at) = comment(&sig, Style::Block, "    ");
        assert_eq!(lines, ["    /**", "     * ", "     *", "     * @param a ", "     * @param b ", "     * @return ", "     */"]);
        assert_eq!(at, 1);
        let (py, at) = comment(&sig, Style::Python, "    ");
        assert_eq!(py, ["    \"\"\"", "", "    Args:", "        a: ", "        b: ", "", "    Returns:", "        ", "    \"\"\""]);
        assert_eq!(at, 0);
        let (cs, _) = comment(&sig, Style::CSharp, "");
        assert!(cs.contains(&"/// <param name=\"b\"></param>".to_string()));
        assert_eq!(comment(&sig, Style::Go, "").0, ["// mix "]);
    }
}
