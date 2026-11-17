//! `tf!`: `format!` for words people read, translated. It takes what `format!` takes; in English, or where there's
//! no translation, it's `format!` exactly. Where there is one, the translation's placeholders (each written as in
//! the English: `{name}`, `{}`, `{count:>4}`) are filled with the same values, in whatever order the translation
//! puts them (unnamed `{}` in the order they come).

use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};

/// The placeholders in a format string, as written: `{name}`, `{0:x}`, `{}`; `{{` and `}}` aren't.
fn placeholders(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' if chars.get(i + 1) == Some(&'{') => i += 2,
            '}' if chars.get(i + 1) == Some(&'}') => i += 2,
            '{' => {
                let start = i;
                while i < chars.len() && chars[i] != '}' {
                    i += 1;
                }
                out.push(chars[start..=i.min(chars.len() - 1)].iter().collect());
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// The text of a string literal's token: its contents, escapes undone as far as format strings need.
fn unquote(literal: &str) -> Option<String> {
    let inner = literal.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '0' => out.push('\0'),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            '\'' => out.push('\''),
            '\n' => {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
            }
            'u' => {
                let mut hex = String::new();
                chars.next();
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                    hex.push(c);
                }
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
            }
            'x' => {
                let hex: String = [chars.next()?, chars.next()?].iter().collect();
                out.push(char::from(u8::from_str_radix(&hex, 16).ok()?));
            }
            _ => return None,
        }
    }
    Some(out)
}

fn punct(c: char) -> TokenTree {
    TokenTree::Punct(Punct::new(c, Spacing::Alone))
}

fn joint(c: char) -> TokenTree {
    TokenTree::Punct(Punct::new(c, Spacing::Joint))
}

fn ident(name: &str) -> TokenTree {
    TokenTree::Ident(Ident::new(name, Span::call_site()))
}

fn path(parts: &[&str]) -> Vec<TokenTree> {
    let mut out = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push(joint(':'));
            out.push(punct(':'));
        }
        out.push(ident(p));
    }
    out
}

fn group(delimiter: Delimiter, tokens: Vec<TokenTree>) -> TokenTree {
    TokenTree::Group(Group::new(delimiter, tokens.into_iter().collect()))
}

/// A `format!` call: the literal and the arguments, as given.
fn format_call(literal: &TokenTree, args: &[Vec<TokenTree>]) -> Vec<TokenTree> {
    let mut inner = vec![literal.clone()];
    for a in args {
        inner.push(punct(','));
        inner.extend(a.iter().cloned());
    }
    vec![ident("format"), punct('!'), group(Delimiter::Parenthesis, inner)]
}

/// Whether a token is a string literal.
fn is_string(t: &TokenTree) -> bool {
    matches!(t, TokenTree::Literal(l) if l.to_string().starts_with('"'))
}

fn wrapped(l: TokenTree) -> Vec<TokenTree> {
    let mut out = path(&["crate", "i18n", "tr"]);
    out.push(group(Delimiter::Parenthesis, vec![l]));
    out
}

/// A value with the words in it translated: a string literal that's the whole value (or the whole value but for
/// `.to_string()` and its kin), or that's each side of an `if` or a `match` arm, at any depth of braces. Other
/// literals (a method's arguments, another macro's) are left as they are, since the code may need them as written.
fn translated_literals(tokens: Vec<TokenTree>) -> Vec<TokenTree> {
    let whole = |ts: &[TokenTree]| -> bool {
        ts.first().is_some_and(is_string)
            && (ts.len() == 1
                || (ts.len() == 4
                    && matches!(&ts[1], TokenTree::Punct(p) if p.as_char() == '.')
                    && matches!(&ts[2], TokenTree::Ident(i) if ["to_string", "into", "to_owned"].contains(&i.to_string().as_str()))))
    };
    if whole(&tokens) {
        let mut out = wrapped(tokens[0].clone());
        out.extend(tokens[1..].iter().cloned());
        return out;
    }
    let mut out: Vec<TokenTree> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i].clone();
        // A match arm's value: => "words",
        let after_arrow = matches!(out.as_slice(), [.., TokenTree::Punct(a), TokenTree::Punct(b)] if a.as_char() == '=' && b.as_char() == '>');
        match t {
            ref l if after_arrow && is_string(l) => out.extend(wrapped(l.clone())),
            TokenTree::Group(g) if g.delimiter() == Delimiter::Brace => {
                let inner = translated_literals(g.stream().into_iter().collect());
                let mut new = Group::new(Delimiter::Brace, inner.into_iter().collect());
                new.set_span(g.span());
                out.push(TokenTree::Group(new));
            }
            other => out.push(other),
        }
        i += 1;
    }
    out
}

#[proc_macro]
pub fn tf(input: TokenStream) -> TokenStream {
    let tokens: Vec<TokenTree> = input.into_iter().collect();
    // The literal, then the arguments, split at the commas between them (a comma inside brackets is in a group).
    let mut args: Vec<Vec<TokenTree>> = Vec::new();
    let mut current = Vec::new();
    for t in tokens.iter().skip(1) {
        match t {
            TokenTree::Punct(p) if p.as_char() == ',' => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(t.clone()),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    let Some(literal) = tokens.first().cloned() else {
        return "compile_error!(\"tf! needs a format string\")".parse().unwrap();
    };
    let Some(text) = unquote(&literal.to_string()) else {
        return format_call(&literal, &args).into_iter().collect();
    };
    // Each argument evaluated once, bound in a match (which keeps any temporaries for as long as format! would):
    // positional ones by number, named ones by their names.
    let mut values: Vec<TokenTree> = Vec::new();
    let mut binds: Vec<TokenTree> = Vec::new();
    let mut positional: Vec<String> = Vec::new();
    let mut named: Vec<String> = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let is_named = a.len() >= 3 && matches!(&a[0], TokenTree::Ident(_)) && matches!(&a[1], TokenTree::Punct(p) if p.as_char() == '=') && !matches!(&a[2], TokenTree::Punct(p) if p.as_char() == '=');
        let (local, value): (String, Vec<TokenTree>) = if is_named { (a[0].to_string(), a[2..].to_vec()) } else { (format!("__tf_{i}"), a.clone()) };
        // Words given as values (", running", or each side of an if) are translated too.
        let value = translated_literals(value);
        if is_named {
            named.push(local.clone());
        } else {
            positional.push(local.clone());
        }
        values.push(punct('&'));
        values.push(group(Delimiter::Parenthesis, value));
        values.push(punct(','));
        binds.push(ident(&local));
        binds.push(punct(','));
    }
    // Each placeholder's value, formatted as the English would format it.
    let mut pieces: Vec<TokenTree> = Vec::new();
    let mut next = 0;
    for p in placeholders(&text) {
        let body = &p[1..p.len() - 1];
        let (name, spec) = body.split_once(':').map_or((body, ""), |(n, s)| (n, s));
        let local = if name.is_empty() {
            let l = positional.get(next).cloned();
            next += 1;
            l
        } else if let Ok(n) = name.parse::<usize>() {
            positional.get(n).cloned()
        } else {
            Some(name.to_string())
        };
        let Some(local) = local else { continue };
        let mut lit = Literal::string(&format!("{{:{spec}}}"));
        lit.set_span(Span::call_site());
        let value = vec![ident("format"), punct('!'), group(Delimiter::Parenthesis, vec![TokenTree::Literal(lit), punct(','), ident(&local)])];
        let mut key = Literal::string(&p);
        key.set_span(Span::call_site());
        pieces.push(group(Delimiter::Parenthesis, vec![TokenTree::Literal(key), punct(','), group(Delimiter::Brace, value)]));
        pieces.push(punct(','));
    }
    // match crate::i18n::translation(literal) { None => format!(literal, args), Some(t) => crate::i18n::fill(t, &[pieces]) }
    let mut english_args = Vec::new();
    for l in &positional {
        english_args.push(vec![ident(l)]);
    }
    for n in &named {
        english_args.push(vec![ident(n), punct('='), ident(n)]);
    }
    let mut body = Vec::new();
    body.push(ident("match"));
    body.extend(path(&["crate", "i18n", "translation"]));
    body.push(group(Delimiter::Parenthesis, vec![literal.clone()]));
    let mut arms = vec![ident("None"), joint('='), punct('>')];
    arms.extend(format_call(&literal, &english_args));
    arms.push(punct(','));
    arms.push(ident("Some"));
    arms.push(group(Delimiter::Parenthesis, vec![ident("__tf_t")]));
    arms.extend([joint('='), punct('>')]);
    arms.extend(path(&["crate", "i18n", "fill"]));
    arms.push(group(Delimiter::Parenthesis, vec![ident("__tf_t"), punct(','), punct('&'), group(Delimiter::Bracket, pieces)]));
    arms.push(punct(','));
    body.push(group(Delimiter::Brace, arms));
    // match (&(a), &(b),) { (__tf_0, __tf_1,) => { match translation { ... } } }
    let outer = vec![
        ident("match"),
        group(Delimiter::Parenthesis, values),
        group(Delimiter::Brace, vec![group(Delimiter::Parenthesis, binds), joint('='), punct('>'), group(Delimiter::Brace, body)]),
    ];
    vec![group(Delimiter::Brace, outer)].into_iter().collect()
}
