//! KickAssembler's and 64tass's ways of writing 6502 source, read by Limoniter's own assembler: each line is
//! rewritten into the assembler's own forms (one line for one, so lines and their numbers stay where they are),
//! enough for everyday C64 programs to assemble to the same bytes, and for hover, completion and the Functions
//! panel to know their labels. What's beyond it (KickAssembler's script language, 64tass's sections) is left to
//! the real assembler, chosen in File, Run on, Assemblers.
//!
//! KickAssembler: .pc = and * = with a segment name, :BasicUpstart2(start), .byte, .word, .dword, .text (in the
//! screen codes it uses by default, or the .encoding chosen), .fill, .align, .const, .var and .label, { } scopes,
//! !name: multi labels and !name- / !name+, label:#$00 argument labels, .if, .for (counting), .macro and its
//! calls, .pseudopc, .import source, binary and c64, and the built-in colour names.
//!
//! 64tass: *=, .byte, .char, .word, .addr, .rta, .long, .dword, .text, .null, .ptext and .shift (as 64tass -a
//! makes them, in PETSCII), .fill, .align, .proc/.pend (left out when nothing uses it, as 64tass does), .block/
//! .bend, anonymous - and + (-- being the - before last), .logical/.here, .if/.elsif/.else/.fi, .rept/.next,
//! .for, .include and .binary.

use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    KickAssembler,
    Tass64,
}

impl Dialect {
    pub fn name(self) -> &'static str {
        match self {
            Dialect::KickAssembler => "KickAssembler",
            Dialect::Tass64 => "64tass",
        }
    }
}

/// KickAssembler's built-in colour names.
const COLOURS: [(&str, i64); 19] = [
    ("BLACK", 0),
    ("WHITE", 1),
    ("RED", 2),
    ("CYAN", 3),
    ("PURPLE", 4),
    ("GREEN", 5),
    ("BLUE", 6),
    ("YELLOW", 7),
    ("ORANGE", 8),
    ("BROWN", 9),
    ("LIGHT_RED", 10),
    ("DARK_GRAY", 11),
    ("DARK_GREY", 11),
    ("GRAY", 12),
    ("GREY", 12),
    ("LIGHT_GREEN", 13),
    ("LIGHT_BLUE", 14),
    ("LIGHT_GRAY", 15),
    ("LIGHT_GREY", 15),
];

/// The line without a // comment (KickAssembler's; its ; isn't one, as .for uses it).
fn without_slashes(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quote: Option<u8> = None;
    for i in 0..bytes.len() {
        match quote {
            Some(q) if bytes[i] == q => quote = None,
            Some(_) => {}
            None if bytes[i] == b'"' => quote = Some(b'"'),
            None if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') => return &line[..i],
            None => {}
        }
    }
    line
}

/// The first word of a statement, in lower case.
fn first_word(code: &str) -> String {
    code.split(|c: char| c.is_whitespace() || c == '(' || c == '{').next().unwrap_or("").to_ascii_lowercase()
}

/// Which way a source is written, from what's in it (or its file's ending): None when it's neither's.
pub fn dialect(path: &Path, text: &str) -> Option<Dialect> {
    let ending = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if matches!(ending.as_str(), "tas" | "64tass") {
        return Some(Dialect::Tass64);
    }
    if matches!(ending.as_str(), "kick" | "ka" | "kickass") {
        return Some(Dialect::KickAssembler);
    }
    let (mut kick, mut tass) = (0, 0);
    for line in text.lines().take(4000) {
        let code = without_slashes(line).trim();
        let code = code.split(';').next().unwrap_or("").trim();
        if code.is_empty() {
            continue;
        }
        let word = first_word(code);
        let label_then = code.split_whitespace().nth(1).map(str::to_ascii_lowercase).unwrap_or_default();
        if matches!(word.as_str(), ".pc" | ".const" | ".label" | "#import" | "#importonce" | ".encoding" | ".pseudopc" | ".eval" | ".print" | ".filenamespace" | ".segment" | ".pseudocommand" | ".function" | ":basicupstart" | ":basicupstart2" | "basicupstart2")
            || word == ".var" && code.contains('=')
            || word == ".import" && code.split_whitespace().nth(1).is_some_and(|w| matches!(w, "source" | "binary" | "c64" | "text"))
            || code.starts_with('!') && code[1..].split(':').next().is_some_and(|n| n.chars().all(|c| c.is_alphanumeric() || c == '_')) && code.contains(':') && !code.contains(' ') || code.starts_with('!') && code.find(':').is_some_and(|c| code[1..c].chars().all(|ch| ch.is_alphanumeric() || ch == '_'))
            || code.contains("!-") || code.contains("!+")
        {
            kick += 1;
        }
        if matches!(word.as_str(), ".pend" | ".bend" | ".endblock" | ".null" | ".logical" | ".here" | ".endlogical" | ".fi" | ".enc" | ".cpu" | ".binclude" | ".rta" | ".shift" | ".ptext" | ".cerror" | ".cwarn" | ".elsif" | ".dsection" | ".send")
            || matches!(label_then.as_str(), ".proc" | ".block" | ".macro" | ".var" | ".null" | ".ptext" | ".shift" | ".logical")
        {
            tass += 1;
        }
    }
    match (kick, tass) {
        (0, 0) => None,
        (k, t) if k >= t => Some(Dialect::KickAssembler),
        _ => Some(Dialect::Tass64),
    }
}

/// Source written one of the ways, as Limoniter's assembler reads it.
pub fn rewrite(dialect: Dialect, text: &str) -> String {
    match dialect {
        Dialect::KickAssembler => kickassembler(text),
        Dialect::Tass64 => tass64(text),
    }
}

/// The name a label was given in its own source, from what it's called here: .name in a scope is name, and a
/// multi or anonymous label's number is taken off.
pub fn written_name(name: &str) -> &str {
    let name = name.trim_start_matches('.');
    match name.find("__m") {
        Some(at) if name[at + 3..].chars().all(|c| c.is_ascii_digit()) => &name[..at],
        _ => name,
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The identifier a text starts with.
fn ident(text: &str) -> &str {
    if !text.starts_with(is_ident_start) {
        return "";
    }
    let end = text.find(|c: char| !is_ident(c)).unwrap_or(text.len());
    &text[..end]
}

/// Each name in an expression (outside quotes, not a $hex or %binary number, not after a dot), passed to `with`,
/// which can give another text for it.
fn rewrite_names(text: &str, with: &dyn Fn(&str) -> Option<String>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            out.push(c);
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
            out.push(c);
            i += 1;
            continue;
        }
        if c == '$' || c == '%' || c.is_ascii_digit() {
            // A number, all of it.
            out.push(c);
            i += 1;
            while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if is_ident_start(c) && (i == 0 || !(chars[i - 1] == '.' || is_ident(chars[i - 1]))) {
            let start = i;
            while i < chars.len() && (is_ident(chars[i]) || chars[i] == '.' && chars.get(i + 1).is_some_and(|n| is_ident_start(*n))) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match with(&word) {
                Some(other) => out.push_str(&other),
                None => out.push_str(&word),
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// A string literal's text, if an item is one.
fn string_of(item: &str) -> Option<&str> {
    let item = item.trim();
    (item.len() >= 2 && item.starts_with('"') && item.ends_with('"')).then(|| &item[1..item.len() - 1])
}

/// The items of a list, split at commas outside quotes and brackets.
fn items(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut quote = None;
    let mut current = String::new();
    for c in text.chars() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' => quote = Some(c),
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                ',' if depth == 0 => {
                    out.push(current.trim().to_string());
                    current.clear();
                    continue;
                }
                _ => {}
            },
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

/// A label's number among those of its name defined, and so the name it's given: loop__m2.
fn numbered(name: &str, n: usize) -> String {
    format!("{}__m{n}", if name.is_empty() { "anon" } else { name })
}

/// Which of a name's definitions a reference on a line means: `back` of them before it (the line's own
/// counting), or `ahead` after it.
fn pick(defined: &[usize], line: usize, back: usize, ahead: usize) -> Option<usize> {
    if back > 0 {
        let before: Vec<usize> = (0..defined.len()).filter(|i| defined[*i] <= line).collect();
        before.len().checked_sub(back).map(|i| before[i])
    } else {
        let after: Vec<usize> = (0..defined.len()).filter(|i| defined[*i] > line).collect();
        after.get(ahead - 1).copied()
    }
}

/// A block a { opened (KickAssembler) or a directive (64tass), and what closes it.
#[derive(Clone, Debug, PartialEq)]
enum Block {
    /// A scope: its labels are its own. The number says which, to find its labels.
    Scope(usize),
    If,
    For,
    Macro(usize),
    Pseudo,
    /// Lines left out: a function, a struct, a 64tass .proc nothing uses, a .comment.
    Skip,
    Plain,
}

/// What a scope's labels are called within it: as its own local labels, under the label it starts with.
struct Scopes {
    /// For each scope, the names defined in it.
    labels: Vec<HashSet<String>>,
    /// For each scope that's a macro, its parameters.
    params: HashMap<usize, Vec<String>>,
}

impl Scopes {
    /// The outermost scope open, whose locals the names in any inner one become.
    fn outer(stack: &[Block]) -> Option<usize> {
        stack.iter().find_map(|b| match b {
            Block::Scope(n) | Block::Macro(n) => Some(*n),
            _ => None,
        })
    }

    /// A name used in a scope: its own local label if it's one of the scope's.
    fn name(&self, stack: &[Block], name: &str) -> Option<String> {
        let scope = Self::outer(stack)?;
        self.labels[scope].contains(name).then(|| format!(".{name}"))
    }
}

// ---------- KickAssembler ----------

/// How a KickAssembler line opens a block, from what's before its {.
fn kick_block(head: &str, scopes: &mut usize) -> Block {
    let word = first_word(head);
    match word.as_str() {
        ".if" => Block::If,
        ".for" => Block::For,
        ".macro" | ".pseudocommand" => {
            *scopes += 1;
            Block::Macro(*scopes - 1)
        }
        ".pseudopc" => Block::Pseudo,
        ".function" | ".struct" | ".enum" | ".while" | ".define" | ".plugin" => Block::Skip,
        _ if word.starts_with('.') || word.starts_with('#') => Block::Plain,
        _ => {
            *scopes += 1;
            Block::Scope(*scopes - 1)
        }
    }
}

/// The labels each line defines: name: (not a multi label), and .const, .var and .label names.
fn kick_defines(code: &str) -> Option<String> {
    let code = code.trim();
    let word = first_word(code);
    if matches!(word.as_str(), ".const" | ".var" | ".label") {
        let rest = code[word.len()..].trim_start();
        let name = ident(rest);
        return (!name.is_empty()).then(|| name.to_string());
    }
    let name = ident(code);
    (!name.is_empty() && code[name.len()..].starts_with(':')).then(|| name.to_string())
}

/// `.for (var i=0; i<8; i++)` as Limoniter's FOR: the variable, the first and the last values.
fn kick_for(head: &str) -> Option<String> {
    let open = head.find('(')?;
    let close = head.rfind(')')?;
    let parts: Vec<&str> = head[open + 1..close].split(';').map(str::trim).collect();
    let [init, test, step] = parts[..] else { return None };
    let init = init.strip_prefix("var ").unwrap_or(init).trim();
    let (name, first) = init.split_once('=')?;
    let name = name.trim();
    let step = step.replace(' ', "");
    if step != format!("{name}++") && step != format!("{name}+=1") && step != format!("{name}={name}+1") {
        return None;
    }
    let test = test.replace(' ', "");
    let last = if let Some(limit) = test.strip_prefix(&format!("{name}<=")) {
        limit.to_string()
    } else {
        format!("({})-1", test.strip_prefix(&format!("{name}<"))?)
    };
    Some(format!(" for {name}, {}, {last}", first.trim()))
}

/// .text in an encoding: Limoniter's directive for it, and how the text's case is put first.
fn kick_text(encoding: &str, operands: &str) -> String {
    let list: Vec<String> = items(operands)
        .into_iter()
        .map(|item| match string_of(&item) {
            Some(text) => text.bytes().map(|c| format!("${:02x}", kick_encode(encoding, c))).collect::<Vec<_>>().join(","),
            None => item,
        })
        .collect();
    format!(" byte {}", list.join(", "))
}

/// A character in one of KickAssembler's encodings, as KickAssembler 5.25 makes it.
fn kick_encode(encoding: &str, c: u8) -> u8 {
    match encoding.to_ascii_lowercase().as_str() {
        "petscii_mixed" => match c {
            b'a'..=b'z' => c - 32,
            b'A'..=b'Z' => c + 128,
            _ => c,
        },
        "petscii_upper" | "ascii" => c,
        "screencode_upper" => match c {
            0x40..=0x5F => c - 0x40,
            _ => c,
        },
        // screencode_mixed, the one it starts with.
        _ => match c {
            b'a'..=b'z' => c - 0x60,
            b'A'..=b'Z' => c,
            0x40..=0x5F => c - 0x40,
            _ => c,
        },
    }
}

fn kickassembler(text: &str) -> String {
    let codes: Vec<String> = text.lines().map(|l| without_slashes(l).trim_end().to_string()).collect();
    // Multi labels: each name's definitions, by line.
    let mut multi: HashMap<String, Vec<usize>> = HashMap::new();
    let mut macros: HashSet<String> = HashSet::new();
    let mut defined: HashSet<String> = HashSet::new();
    for (n, code) in codes.iter().enumerate() {
        let code = code.trim();
        if let Some(rest) = code.strip_prefix('!') {
            let name = ident(rest);
            if rest[name.len()..].starts_with(':') {
                multi.entry(name.to_string()).or_default().push(n);
            }
        }
        if first_word(code) == ".macro" {
            let name = ident(code[6..].trim_start().trim_start_matches('@'));
            macros.insert(name.to_ascii_lowercase());
        }
        if let Some(name) = kick_defines(code) {
            defined.insert(name);
        }
    }
    // The scopes, and the labels of each; and the number of the scope or macro each line opens.
    let mut scopes = Scopes { labels: Vec::new(), params: HashMap::new() };
    let mut count = 0;
    let mut numbers: HashMap<usize, usize> = HashMap::new();
    let mut stack: Vec<Block> = Vec::new();
    for (n, code) in codes.iter().enumerate() {
        let code = code.trim();
        if code.starts_with('}') {
            stack.pop();
            if code.ends_with('{') && code.contains("else") {
                stack.push(Block::If);
            }
            continue;
        }
        if let Some(name) = kick_defines(code)
            && let Some(scope) = Scopes::outer(&stack)
        {
            scopes.labels[scope].insert(name);
        }
        if let Some(head) = code.strip_suffix('{') {
            let block = kick_block(head.trim(), &mut count);
            scopes.labels.resize(count, HashSet::new());
            if let Block::Scope(number) | Block::Macro(number) = block {
                numbers.insert(n, number);
            }
            if let (Block::Macro(number), Some(open), Some(close)) = (&block, head.find('('), head.rfind(')')) {
                scopes.params.insert(*number, items(&head[open + 1..close]));
            }
            stack.push(block);
        }
    }
    // Then each line, rewritten.
    let mut out = String::with_capacity(text.len());
    let mut stack: Vec<Block> = Vec::new();
    let mut encoding = "screencode_mixed".to_string();
    let mut hidden = 0;
    for (n, code) in codes.iter().enumerate() {
        let code = code.trim();
        let line = if let Some(after) = code.strip_prefix('}') {
            let after = after.trim();
            let closed = stack.pop();
            if after.ends_with('{') && after.starts_with("else") {
                stack.push(Block::If);
                let condition = after["else".len()..after.len() - 1].trim();
                match condition.strip_prefix(".if").or_else(|| condition.strip_prefix("if")) {
                    Some(c) => format!(" elseif {}", c.trim()),
                    None => " else".to_string(),
                }
            } else {
                match closed {
                    Some(Block::If) => " endif".into(),
                    Some(Block::For) => " endr".into(),
                    Some(Block::Macro(_)) => " endm".into(),
                    Some(Block::Pseudo) => " dephase".into(),
                    _ => String::new(),
                }
            }
        } else if stack.contains(&Block::Skip) {
            if code.ends_with('{') {
                stack.push(Block::Skip);
            }
            String::new()
        } else if let Some(head) = code.strip_suffix('{') {
            let head = head.trim();
            let mut throwaway = 0;
            let block = kick_block(head, &mut throwaway);
            let word = first_word(head);
            let said = match &block {
                Block::If => format!(" if {}", head[3..].trim()),
                Block::For => kick_for(head).unwrap_or_else(|| " error \"this .for isn't one Limoniter's assembler can count\"".into()),
                Block::Macro(_) => {
                    let rest = head[word.len()..].trim().trim_start_matches('@');
                    let name = ident(rest);
                    let params = rest[name.len()..].trim().trim_start_matches('(').trim_end_matches(')').to_string();
                    format!(" macro {name} {params}")
                }
                Block::Pseudo => format!(" phase {}", head[".pseudopc".len()..].trim()),
                Block::Scope(_) => {
                    let name = ident(head);
                    if !name.is_empty() && head[name.len()..].trim() == ":" {
                        match Scopes::outer(&stack) {
                            Some(_) => format!(" .{name}:"),
                            None => format!("{name}:"),
                        }
                    } else if Scopes::outer(&stack).is_none() {
                        hidden += 1;
                        format!("scope__m{hidden}:")
                    } else {
                        String::new()
                    }
                }
                _ => String::new(),
            };
            // The same numbering as the first pass, for the labels.
            let numbered_block = match block {
                Block::Scope(_) => Block::Scope(numbers.get(&n).copied().unwrap_or(0)),
                Block::Macro(_) => Block::Macro(numbers.get(&n).copied().unwrap_or(0)),
                other => other,
            };
            stack.push(numbered_block);
            said
        } else {
            kick_statement(code, n, &stack, &scopes, &multi, &macros, &defined, &mut encoding)
        };
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn kick_statement(code: &str, line: usize, stack: &[Block], scopes: &Scopes, multi: &HashMap<String, Vec<usize>>, macros: &HashSet<String>, defined: &HashSet<String>, encoding: &mut String) -> String {
    let mut code = code.trim().to_string();
    let mut label = String::new();
    // !name: a multi label.
    if let Some(rest) = code.strip_prefix('!') {
        let name = ident(rest);
        if let Some(after) = rest[name.len()..].strip_prefix(':') {
            let index = multi.get(name).and_then(|lines| lines.iter().position(|l| *l == line)).unwrap_or(0);
            label = format!("{}{}: ", if Scopes::outer(stack).is_some() { " ." } else { "" }, numbered(name, index));
            code = after.trim().to_string();
        }
    }
    // name: a label, its scope's own inside one.
    let name = ident(&code).to_string();
    if !name.is_empty() && code[name.len()..].starts_with(':') && !code[name.len()..].starts_with("::") {
        label.push_str(&match Scopes::outer(stack) {
            Some(_) => format!(" .{name}: "),
            None => format!("{name}: "),
        });
        code = code[name.len() + 1..].trim().to_string();
    }
    let names = |text: &str| -> String {
        // Multi label references first: !name- and !name+.
        let mut rewritten = String::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '!' && !matches!(chars.get(i + 1), Some('=')) {
                let rest: String = chars[i + 1..].iter().collect();
                let name = ident(&rest);
                let signs: String = rest[name.len()..].chars().take_while(|c| *c == '+' || *c == '-').collect();
                if !signs.is_empty() && signs.chars().all(|c| c == signs.chars().next().unwrap()) {
                    let (back, ahead) = if signs.starts_with('-') { (signs.len(), 0) } else { (0, signs.len()) };
                    let lines = multi.get(name).map(Vec::as_slice).unwrap_or_default();
                    match pick(lines, line, back, ahead) {
                        Some(index) => rewritten.push_str(&format!("{}{}", if Scopes::outer(stack).is_some() { "." } else { "" }, numbered(name, index))),
                        None => rewritten.push_str(&format!("{}__missing", if name.is_empty() { "anon" } else { name })),
                    }
                    i += 1 + name.len() + signs.len();
                    continue;
                }
            }
            rewritten.push(chars[i]);
            i += 1;
        }
        rewrite_names(&rewritten, &|word| {
            if let Some(local) = scopes.name(stack, word) {
                return Some(local);
            }
            if !defined.contains(word)
                && let Some((_, v)) = COLOURS.iter().find(|(name, _)| *name == word)
            {
                return Some(v.to_string());
            }
            None
        })
    };
    // { lda #1 } on one line: a scope of its own, which changes nothing here.
    if code.starts_with('{') && code.ends_with('}') {
        code = code[1..code.len() - 1].trim().to_string();
    }
    if code.is_empty() {
        return label;
    }
    let word = if code.starts_with('*') { "*".to_string() } else { first_word(&code) };
    let rest = code[word.len().min(code.len())..].trim().to_string();
    let said = match word.as_str() {
        ".pc" | "*" | "*=" => {
            let value = code.trim_start_matches(".pc").trim_start_matches('*').trim_start().trim_start_matches('=').trim();
            // The segment's name after the address, in quotes, is only a name.
            let value = match value.find('"') {
                Some(at) => value[..at].trim(),
                None => value,
            };
            format!(" *= {}", names(value))
        }
        ".const" | ".var" | ".label" => {
            let name = ident(&rest);
            let value = rest[name.len()..].trim().trim_start_matches('=').trim();
            let shown = if Scopes::outer(stack).is_some() { format!(".{name}") } else { name.to_string() };
            format!(" {shown} = {}", names(value))
        }
        ".byte" | ".by" => format!(" byte {}", names(&rest)),
        ".word" | ".wo" => format!(" word {}", names(&rest)),
        ".dword" | ".dw" => format!(" dd {}", names(&rest)),
        ".text" | ".te" => kick_text(encoding, &names(&rest)),
        ".fill" => format!(" fill {}", names(&rest)),
        ".align" => format!(" align {}", names(&rest)),
        ".encoding" => {
            *encoding = string_of(&rest).unwrap_or("screencode_mixed").to_string();
            String::new()
        }
        "#import" | ".import" => {
            let (kind, file) = match rest.split_once(char::is_whitespace) {
                Some((kind, file)) if !kind.starts_with('"') => (kind.to_ascii_lowercase(), file.trim().to_string()),
                _ => ("source".to_string(), rest.clone()),
            };
            match kind.as_str() {
                "source" => format!(" include {file}"),
                "binary" => format!(" incbin {file}"),
                "c64" => {
                    let parts = items(&file);
                    let skip = parts.get(1).map(|s| format!("2+({s})")).unwrap_or_else(|| "2".into());
                    let length = parts.get(2).map(|l| format!(", {l}")).unwrap_or_default();
                    format!(" incbin {}, {skip}{length}", parts.first().cloned().unwrap_or_default())
                }
                _ => String::new(),
            }
        }
        // What makes no bytes, or isn't the assembler's to follow.
        "#importonce" | "#define" | "#undef" | "#if" | "#elif" | "#else" | "#endif" | ".print" | ".printnow" | ".error" | ".errorif" | ".assert" | ".asserterror" | ".watch" | ".break" | ".segment" | ".segmentdef" | ".segmentout" | ".file" | ".disk" | ".plugin" | ".cpu" | ".filenamespace" | ".namespace" | ".eval" | ".modify" | ".zp" | ".memblock" | ".importonce" => String::new(),
        _ => {
            // A macro: :Name(arguments), or Name(arguments) for one the source defines.
            let call = code.strip_prefix(':').unwrap_or(&code);
            let called = ident(call);
            let lower = called.to_ascii_lowercase();
            if !called.is_empty() && call[called.len()..].trim_start().starts_with('(') && (code.starts_with(':') || macros.contains(&lower) || lower.starts_with("basicupstart")) {
                let arguments = call[called.len()..].trim().trim_start_matches('(').trim_end_matches(')');
                if lower == "basicupstart2" && !macros.contains(&lower) {
                    format!(" basicupstart2 {}", names(arguments))
                } else if lower == "basicupstart" && !macros.contains(&lower) {
                    format!(" basicupstart {}", names(arguments))
                } else {
                    format!(" {called} {}", names(arguments))
                }
            } else if let Some((name, after)) = argument_label(&code) {
                // lda name:#$00: a label on the instruction's argument, one byte in.
                let shown = if Scopes::outer(stack).is_some() { format!(".{name}") } else { name.to_string() };
                format!(" {shown} = *+1 : {}", names(&after))
            } else {
                format!(" {}", names(&code))
            }
        }
    };
    // A macro's parameter straight after # is put in brackets, as the assembler takes #name for one word.
    let mut said = said;
    if let Some(params) = stack.iter().rev().find_map(|b| if let Block::Macro(n) = b { scopes.params.get(n) } else { None }) {
        for p in params {
            said = rewrite_hash(&said, p);
        }
    }
    format!("{label}{said}")
}

/// #name as #(name), for a macro's parameter.
fn rewrite_hash(text: &str, name: &str) -> String {
    let pattern = format!("#{name}");
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(&pattern) {
        let after = &rest[at + pattern.len()..];
        out.push_str(&rest[..at]);
        if after.starts_with(is_ident) {
            out.push_str(&pattern);
        } else {
            out.push_str(&format!("#({name})"));
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// An instruction whose argument has a label in front of it: lda tmp:#$00. The label, and the instruction without
/// it.
fn argument_label(code: &str) -> Option<(String, String)> {
    let (mnemonic, operand) = code.split_once(char::is_whitespace)?;
    let operand = operand.trim();
    let name = ident(operand);
    if mnemonic.len() != 3 || name.is_empty() {
        return None;
    }
    let after = operand[name.len()..].strip_prefix(':')?;
    Some((name.to_string(), format!("{mnemonic} {}", after.trim())))
}

// ---------- 64tass ----------

/// A 64tass line: its label (in the first column, or before a directive), and the rest.
fn tass_split(code: &str) -> (Option<&str>, &str) {
    let at_start = !code.starts_with([' ', '\t']);
    let trimmed = code.trim_start();
    let name = ident(trimmed);
    if name.is_empty() {
        return (None, code.trim());
    }
    let after = &trimmed[name.len()..];
    if let Some(rest) = after.strip_prefix(':') {
        return (Some(name), rest.trim());
    }
    let next = after.trim_start();
    let is_instruction = crate::asm::m6502::M6502.is_instruction(&name.to_ascii_lowercase());
    if (at_start && !is_instruction) || next.starts_with('.') && !trimmed.starts_with('.') && after.starts_with(char::is_whitespace) {
        return (Some(name), next);
    }
    (None, code.trim())
}

use crate::asm::Isa;

fn tass64(text: &str) -> String {
    let codes: Vec<String> = text.lines().map(|l| crate::asm::strip_comment(l).trim_end().to_string()).collect();
    // Anonymous labels: where each - and + is.
    let mut minus: Vec<usize> = Vec::new();
    let mut plus: Vec<usize> = Vec::new();
    for (n, code) in codes.iter().enumerate() {
        match code.split_whitespace().next() {
            Some("-") => minus.push(n),
            Some("+") => plus.push(n),
            _ => {}
        }
    }
    // The procedures used: those named anywhere but where they're defined.
    let words: HashMap<String, usize> = {
        let mut seen = HashMap::new();
        for code in &codes {
            for w in code.split(|c: char| !is_ident(c)) {
                if !w.is_empty() {
                    *seen.entry(w.to_string()).or_insert(0) += 1;
                }
            }
        }
        seen
    };
    // The scopes, and their labels.
    let mut scopes = Scopes { labels: Vec::new(), params: HashMap::new() };
    let mut stack: Vec<Block> = Vec::new();
    let mut opened: Vec<Block> = Vec::new();
    for code in &codes {
        let (label, rest) = tass_split(code);
        let word = first_word(rest);
        match word.as_str() {
            ".proc" | ".block" => {
                if let Some(name) = label
                    && let Some(scope) = Scopes::outer(&stack)
                {
                    scopes.labels[scope].insert(name.to_string());
                }
                let unused = word == ".proc" && label.is_some_and(|name| words.get(name).copied().unwrap_or(0) <= 1);
                let block = if unused { Block::Skip } else { Block::Scope(scopes.labels.len()) };
                if !unused {
                    scopes.labels.push(HashSet::new());
                }
                stack.push(block.clone());
                opened.push(block);
                continue;
            }
            ".pend" | ".endproc" | ".bend" | ".endblock" => {
                stack.pop();
                continue;
            }
            _ => {}
        }
        if let Some(name) = label
            && let Some(scope) = Scopes::outer(&stack)
            && !stack.contains(&Block::Skip)
        {
            scopes.labels[scope].insert(name.to_string());
        }
    }
    let mut opened = opened.into_iter();
    let mut out = String::with_capacity(text.len());
    let mut stack: Vec<Block> = Vec::new();
    let mut screen = false;
    let mut hidden = 0;
    let mut comment = false;
    for (n, code) in codes.iter().enumerate() {
        let (label, rest) = tass_split(code);
        let word = first_word(rest);
        let operands = rest[word.len().min(rest.len())..].trim();
        if comment {
            if word == ".endc" || word == ".endcomment" {
                comment = false;
            }
            out.push('\n');
            continue;
        }
        let skipping = stack.contains(&Block::Skip);
        let line = match word.as_str() {
            ".proc" | ".block" => {
                let block = opened.next().unwrap_or(Block::Plain);
                let said = match (&block, label) {
                    (Block::Skip, _) => String::new(),
                    (_, Some(name)) if Scopes::outer(&stack).is_some() => format!(" .{name}:"),
                    (_, Some(name)) => format!("{name}:"),
                    (_, None) if Scopes::outer(&stack).is_none() => {
                        hidden += 1;
                        format!("block__m{hidden}:")
                    }
                    _ => String::new(),
                };
                stack.push(block);
                if skipping { String::new() } else { said }
            }
            ".pend" | ".endproc" | ".bend" | ".endblock" => {
                stack.pop();
                String::new()
            }
            _ if skipping => String::new(),
            ".comment" => {
                comment = true;
                String::new()
            }
            _ => {
                let anonymous = |item: &str| -> Option<String> {
                    // - and + alone, or in brackets.
                    let (open, body, close) = match item.strip_prefix('(').and_then(|i| i.strip_suffix(')')) {
                        Some(inner) => ("(", inner.trim(), ")"),
                        None => ("", item, ""),
                    };
                    if body.is_empty() || !(body.chars().all(|c| c == '-') || body.chars().all(|c| c == '+')) {
                        return None;
                    }
                    let (back, ahead) = if body.starts_with('-') { (body.len(), 0) } else { (0, body.len()) };
                    let (defined, name) = if back > 0 { (&minus, "minus") } else { (&plus, "plus") };
                    Some(match pick(defined, n, back, ahead) {
                        Some(i) => format!("{open}{}{close}", numbered(name, i)),
                        None => format!("{open}{name}__missing{close}"),
                    })
                };
                let names = |text: &str| -> String {
                    let parts: Vec<String> = items(text.trim())
                        .into_iter()
                        .map(|item| anonymous(&item).unwrap_or_else(|| rewrite_names(&item, &|w| scopes.name(&stack, w))))
                        .collect();
                    parts.join(", ")
                };
                let label_text = match label {
                    Some(name) if Scopes::outer(&stack).is_some() => format!(" .{name}: "),
                    Some(name) => format!("{name}: "),
                    None => match code.split_whitespace().next() {
                        Some("-") => format!("{}: ", numbered("minus", minus.iter().position(|l| *l == n).unwrap_or(0))),
                        Some("+") => format!("{}: ", numbered("plus", plus.iter().position(|l| *l == n).unwrap_or(0))),
                        _ => String::new(),
                    },
                };
                let rest = match code.split_whitespace().next() {
                    Some("-" | "+") if label.is_none() => code.trim_start()[1..].trim(),
                    _ => rest,
                };
                let word = first_word(rest);
                let operands = if word.len() <= rest.len() { rest[word.len()..].trim() } else { operands };
                let text = |directive: &str, list: &str, after: &str| -> String {
                    let list: Vec<String> = items(list).into_iter().map(|i| if string_of(&i).is_some() { i } else { names(&i) }).collect();
                    format!(" {directive} {}{after}", list.join(", "))
                };
                let said = match word.as_str() {
                    "*=" => format!(" *= {}", names(operands)),
                    "*" if operands.starts_with('=') => format!(" *= {}", names(&operands[1..])),
                    ".byte" | ".char" => format!(" byte {}", names(operands)),
                    ".word" | ".addr" => format!(" word {}", names(operands)),
                    ".rta" => format!(" word {}", items(operands).iter().map(|i| format!("({})-1", names(i))).collect::<Vec<_>>().join(", ")),
                    ".long" => format!(" d24 {}", names(operands)),
                    ".dword" | ".dint" => format!(" dd {}", names(operands)),
                    ".text" => text(if screen { "scr" } else { "pet" }, operands, ""),
                    ".null" => text(if screen { "scr" } else { "pet" }, operands, ", 0"),
                    ".ptext" => {
                        let length: usize = items(operands).iter().map(|i| string_of(i).map_or(1, str::len)).sum();
                        format!(" byte {length} :{}", text(if screen { "scr" } else { "pet" }, operands, ""))
                    }
                    ".shift" => {
                        let list: Vec<String> = items(operands).into_iter().map(|i| string_of(&i).map_or_else(|| names(&i), |s| format!("\"{}\"", s.to_ascii_uppercase()))).collect();
                        format!(" dc {}", list.join(", "))
                    }
                    ".fill" => format!(" fill {}", names(operands)),
                    ".align" => format!(" align {}", names(operands)),
                    ".include" => format!(" include {operands}"),
                    ".binary" => format!(" incbin {operands}"),
                    ".logical" => format!(" phase {}", names(operands)),
                    ".here" | ".endlogical" => " dephase".into(),
                    ".if" => format!(" if {}", names(operands)),
                    ".ifdef" | ".ifndef" => format!(" {} {operands}", &word[1..]),
                    ".elsif" | ".elif" => format!(" elseif {}", names(operands)),
                    ".else" => " else".into(),
                    ".fi" | ".endif" => " endif".into(),
                    ".rept" | ".repeat" => format!(" rept {}", names(operands)),
                    ".next" | ".endrept" | ".endfor" => " endr".into(),
                    ".for" => tass_for(operands).unwrap_or_else(|| " error \"this .for isn't one Limoniter's assembler can count\"".into()),
                    ".var" if label.is_some() => format!(" = {}", names(operands)),
                    ".enc" | ".encode" => {
                        screen = string_of(operands).unwrap_or(operands).eq_ignore_ascii_case("screen");
                        String::new()
                    }
                    ".cpu" | ".cdef" | ".edef" | ".page" | ".endp" | ".endpage" | ".warn" | ".error" | ".cerror" | ".cwarn" | ".assert" | ".check" | ".option" | ".eor" | ".seed" | ".weak" | ".endweak" | ".as" | ".al" | ".xs" | ".xl" | ".autsiz" | ".mansiz" | ".databank" | ".dpage" | ".end" | ".offs" => String::new(),
                    _ if word.starts_with('#') => format!(" {} {}", &word[1..], names(operands)),
                    _ if label.is_some() && rest.trim_start().starts_with(['=', ':']) => format!(" {}", names(rest)),
                    _ if rest.is_empty() => String::new(),
                    // An instruction: its operand's names.
                    _ if crate::asm::m6502::M6502.is_instruction(&word) => format!(" {word} {}", names(operands)),
                    _ => format!(" {}", names(rest)),
                };
                // A label given a value: name = value.
                if let Some(name) = label
                    && (rest.starts_with('=') || rest.starts_with(":="))
                {
                    let value = rest.trim_start_matches(':').trim_start_matches('=').trim();
                    let shown = if Scopes::outer(&stack).is_some() { format!(".{name}") } else { name.to_string() };
                    format!(" {shown} = {}", names(value))
                } else if word == ".var" && let Some(name) = label {
                    let shown = if Scopes::outer(&stack).is_some() { format!(".{name}") } else { name.to_string() };
                    format!(" {shown} = {}", names(operands))
                } else {
                    format!("{label_text}{said}")
                }
            }
        };
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// 64tass's `.for i = 0, i < 8, i += 1` as Limoniter's FOR.
fn tass_for(operands: &str) -> Option<String> {
    let parts = items(operands);
    let [init, test, step] = &parts[..] else { return None };
    let (name, first) = init.split_once('=')?;
    let name = name.trim();
    let step = step.replace(' ', "");
    if step != format!("{name}+=1") && step != format!("{name}={name}+1") {
        return None;
    }
    let test = test.replace(' ', "");
    let last = if let Some(limit) = test.strip_prefix(&format!("{name}<=")) { limit.to_string() } else { format!("({})-1", test.strip_prefix(&format!("{name}<"))?) };
    Some(format!(" for {name}, {}, {last}", first.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assemble(name: &str, text: &str) -> crate::asm::Assembly {
        crate::assemblers::assemble_own(crate::assemblers::Cpu::M6502, Path::new(name), text).assembly
    }

    #[test]
    fn the_dialects_are_told_apart() {
        assert_eq!(dialect(Path::new("a.asm"), ".pc = $0801\n:BasicUpstart2(start)\n"), Some(Dialect::KickAssembler));
        assert_eq!(dialect(Path::new("a.asm"), "!loop:\n  dex\n  bne !loop-\n"), Some(Dialect::KickAssembler));
        assert_eq!(dialect(Path::new("a.asm"), "show .proc\n  rts\n  .pend\n"), Some(Dialect::Tass64));
        assert_eq!(dialect(Path::new("a.asm"), "  *=$c000\n  lda #1\n  !byte 1\n- inx\n  bne -\n"), None, "ACME");
        assert_eq!(dialect(Path::new("a.asm"), ".proc main\n  rts\n.endproc\n"), None, "ca65");
        assert_eq!(dialect(Path::new("demo.tas"), "  lda #1\n"), Some(Dialect::Tass64));
    }

    #[test]
    fn kickassembler_source_assembles_as_kickassembler_makes_it() {
        // Assembled by KickAssembler 5.25: the bytes it made from $0801.
        let text = ".pc = $0801 \"Basic\"\n:BasicUpstart2(start)\n.pc = $0810 \"Code\"\n.const BORDER = $d020\nstart:\n    lda #0\n    sta BORDER\n!loop:\n    inc $d021\n    ldx #10\n    dex\n    bne !loop-\n    jmp !loop-\nmsg: .text \"HELLO\"\n     .byte 1,2,3\n     .word start\n     { lda #1 }\n";
        let assembly = assemble("k.asm", text);
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let made = "0b080a009e32303634000000000000a9008d20d0ee21d0a20acad0f84c150848454c4c4f0102031008a901";
        let bytes = assembly.image(0x0801, assembly.highest().unwrap());
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, made);
        let lines: Vec<(usize, i64)> = assembly.lines.iter().filter(|p| p.address >= 0x0810).map(|p| (p.line, p.address)).collect();
        assert_eq!(lines[..3], [(5, 0x0810), (6, 0x0812), (8, 0x0815)]);
    }

    #[test]
    fn kickassembler_scopes_macros_and_loops() {
        let text = "* = $1000\nmain: {\n  ldx #0\nloop:\n  inx\n  bne loop\n  jmp other.loop\n}\nother: {\nloop: rts\n}\n.macro SetBorder(color) {\n  lda #color\n  sta $d020\n}\n  :SetBorder(RED)\n  SetBorder(BLUE)\n.for (var i=0; i<3; i++) {\n  .byte i*2\n}\n.if (1 == 1) {\n  nop\n} else {\n  brk\n}\n  lda self:#$00\n  .encoding \"petscii_mixed\"\n  .text \"Ab\"\n.function f(x) {\n  .return x\n}\n";
        let assembly = assemble("k.asm", text);
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let bytes = assembly.image(0x1000, assembly.highest().unwrap());
        let mut want = vec![0xA2, 0, 0xE8, 0xD0, 0xFD, 0x4C, 0x08, 0x10, 0x60, 0xA9, 2, 0x8D, 0x20, 0xD0, 0xA9, 6, 0x8D, 0x20, 0xD0, 0, 2, 4, 0xEA, 0xA9, 0];
        want.extend([0xC1, 0x42]);
        assert_eq!(bytes, want);
        assert_eq!(assembly.symbols.get("main.loop"), Some(&0x1002));
        assert_eq!(assembly.symbols.get("self"), Some(&0x1018));
    }

    #[test]
    fn tass64_source_assembles_as_64tass_makes_it() {
        // Assembled by 64tass 1.60 with -a: the bytes it made.
        let text = "        *= $0801\n        .word (+), 10\n        .byte $9e\n        .text \"2061\"\n        .byte 0\n+       .word 0\nBORDER = $d020\nstart   lda #0\n        sta BORDER\nloop    inc $d021\n-       ldx #10\n        dex\n        bne -\n        jmp loop\nmsg     .text \"hello\"\n        .byte 1,2,3\n        .word start\nshow    .proc\n        lda #1\n        rts\n        .pend\nblk     .block\ninner   nop\n        .bend\n        jsr show\n        lda blk.inner\nunused  .proc\n        brk\n        .pend\n";
        let assembly = assemble("t.asm", text);
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let bytes = assembly.image(0x0801, assembly.highest().unwrap());
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "0b080a009e32303631000000a9008d20d0ee21d0a20acad0fb4c120848454c4c4f0102030d08a90160ea202708ad2a08");
        assert_eq!(assembly.symbols.get("blk.inner"), Some(&0x082A));
    }

    #[test]
    fn tass64_anonymous_labels_count_as_64tass_counts_them() {
        let text = "  *= $1000\n-  nop\n-  nop\n   bne --\n   beq +\n   bcc ++\n+  nop\n+  rts\n  .null \"ab\"\n  .enc \"screen\"\n  .text \"ab\"\n";
        let assembly = assemble("t.tas", text);
        assert!(assembly.ok(), "{:?}", assembly.problems);
        assert_eq!(assembly.image(0x1000, assembly.highest().unwrap()), [0xEA, 0xEA, 0xD0, 0xFC, 0xF0, 0x02, 0x90, 0x01, 0xEA, 0x60, 0x41, 0x42, 0, 1, 2]);
    }

    #[test]
    fn names_are_given_back_as_written() {
        assert_eq!(written_name("loop__m3"), "loop");
        assert_eq!(written_name(".inner"), "inner");
        assert_eq!(written_name("border"), "border");
    }
}
