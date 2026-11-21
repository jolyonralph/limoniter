//! Structure templates: a file format described once, in a small language of Limoniter's own, and a file's bytes
//! read with that description into a tree of named fields, each knowing where its bytes are.
//!
//! The language is declarative, as 010 Editor's templates and ImHex's patterns are, but much smaller than either:
//! fields one to a line (`u16 width`), structs, arrays whose counts come from fields read earlier, byte order,
//! enums and flags, strings, `if` and `else`, fields placed at an offset (which is how pointers are followed),
//! and expressions with the usual operators and a few functions for looking ahead in the file. The whole language
//! is described in `data/structures/LANGUAGE.md`, which is also shown as a page from the Structure panel.
//!
//! Nothing here draws anything: `structure_panel.rs` shows the tree and colours the hex view with it.

use std::collections::HashMap;

/// Where a template's bytes come from: a file held whole, or the hex view's buffer, which may keep a large file on
/// disk. Fields are read a few bytes at a time, so a large file is never read whole.
pub trait Source {
    fn len(&self) -> u64;
    /// Up to `count` bytes from `at`; fewer at the end of the file.
    fn read(&self, at: u64, count: usize) -> Vec<u8>;
}

/// Bytes held whole, as a file made by a test.
#[cfg(test)]
pub struct Bytes<'a>(pub &'a [u8]);

#[cfg(test)]
impl Source for Bytes<'_> {
    fn len(&self) -> u64 {
        self.0.len() as u64
    }

    fn read(&self, at: u64, count: usize) -> Vec<u8> {
        let start = (at.min(self.0.len() as u64)) as usize;
        let end = start.saturating_add(count).min(self.0.len());
        self.0[start..end].to_vec()
    }
}

/// The most fields one file is read into, so a template gone wrong (or a damaged file) can't fill the memory.
pub const MOST_NODES: usize = 200_000;
/// How deep structs can sit inside each other, which also stops a struct that includes itself for ever.
const MOST_DEPTH: usize = 64;
/// The most times all the loops in one reading go round, so `[while ...]` can't go on for ever.
const MOST_STEPS: u64 = 20_000_000;
/// How many bytes a zero-terminated string is looked through for its end.
const MOST_STRING: usize = 64 * 1024;
/// How many colours fields take in turn in the hex view.
pub const COLOURS: u8 = 6;

/// Something wrong with a template, or found while reading a file with it: the template's line, and what.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub line: usize,
    pub message: String,
}

fn problem(line: usize, message: String) -> Problem {
    Problem { line, message }
}

// ---------- Reading the template's words ----------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Int(i128),
    Float(f64),
    Str(Vec<u8>),
    Sym(&'static str),
    Newline,
    End,
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    line: usize,
}

const SYMBOLS: [&str; 32] = [
    "<<", ">>", "&&", "||", "==", "!=", "<=", ">=", "{", "}", "(", ")", "[", "]", ",", ".", "@", ":", ";", "?", "+", "-", "*", "/", "%", "&", "|", "^", "~", "!", "<", ">",
];

fn lex(text: &str) -> Result<Vec<Token>, Problem> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut line = 1;
    // Inside round or square brackets a line can go on to the next, so long expressions can be split.
    let mut depth = 0usize;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            if depth == 0 {
                out.push(Token { tok: Tok::Newline, line });
            }
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '#' || (c == '/' && chars.get(i + 1) == Some(&'/')) {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c.is_ascii_digit() || (c == '$' && chars.get(i + 1).is_some_and(char::is_ascii_hexdigit)) {
            let start = i;
            let (radix, skip) = if c == '$' {
                (16, 1)
            } else if c == '0' && matches!(chars.get(i + 1), Some('x' | 'X')) {
                (16, 2)
            } else if c == '0' && matches!(chars.get(i + 1), Some('b' | 'B')) {
                (2, 2)
            } else {
                (10, 0)
            };
            i += skip;
            let digits_start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let mut digits: String = chars[digits_start..i].iter().filter(|c| **c != '_').collect();
            if radix == 10 && chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(char::is_ascii_digit) {
                i += 1;
                digits.push('.');
                while i < chars.len() && chars[i].is_ascii_digit() {
                    digits.push(chars[i]);
                    i += 1;
                }
                let value = digits.parse::<f64>().map_err(|_| problem(line, crate::i18n::tf!("{digits} isn't a number")))?;
                out.push(Token { tok: Tok::Float(value), line });
                continue;
            }
            let written: String = chars[start..i].iter().collect();
            let value = i128::from_str_radix(&digits, radix).map_err(|_| problem(line, crate::i18n::tf!("{written} isn't a number")))?;
            out.push(Token { tok: Tok::Int(value), line });
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Token { tok: Tok::Ident(chars[start..i].iter().collect()), line });
        } else if c == '"' || c == '\'' {
            let quote = c;
            let mut bytes = Vec::new();
            i += 1;
            loop {
                let Some(&ch) = chars.get(i) else {
                    return Err(problem(line, crate::i18n::tr("This text has no closing quote.").into()));
                };
                i += 1;
                if ch == quote {
                    break;
                }
                if ch == '\n' {
                    return Err(problem(line, crate::i18n::tr("This text has no closing quote.").into()));
                }
                if ch == '\\' {
                    let escaped = chars.get(i).copied().unwrap_or(' ');
                    i += 1;
                    match escaped {
                        'n' => bytes.push(b'\n'),
                        'r' => bytes.push(b'\r'),
                        't' => bytes.push(b'\t'),
                        '0' => bytes.push(0),
                        'x' => {
                            let hex: String = chars.get(i..i + 2).map(|h| h.iter().collect()).unwrap_or_default();
                            let value = u8::from_str_radix(&hex, 16).map_err(|_| problem(line, crate::i18n::tr("\\x needs two hex digits after it.").into()))?;
                            bytes.push(value);
                            i += 2;
                        }
                        other => push_char(&mut bytes, other),
                    }
                } else {
                    push_char(&mut bytes, ch);
                }
            }
            // A character in single quotes is its code, as in C.
            if quote == '\'' && bytes.len() == 1 {
                out.push(Token { tok: Tok::Int(i128::from(bytes[0])), line });
            } else {
                out.push(Token { tok: Tok::Str(bytes), line });
            }
        } else if let Some(sym) = SYMBOLS.iter().find(|s| s.chars().zip(&chars[i..]).filter(|(a, b)| a == *b).count() == s.len()) {
            match *sym {
                "(" | "[" => depth += 1,
                ")" | "]" => depth = depth.saturating_sub(1),
                _ => {}
            }
            out.push(Token { tok: Tok::Sym(sym), line });
            i += sym.len();
        } else if c == '=' {
            out.push(Token { tok: Tok::Sym("="), line });
            i += 1;
        } else {
            return Err(problem(line, crate::i18n::tf!("A template can't have '{c}' here.")));
        }
    }
    out.push(Token { tok: Tok::Newline, line });
    out.push(Token { tok: Tok::End, line });
    Ok(out)
}

/// Text in a template stands for bytes: Latin-1 for the first 256 characters, as the files it reads mostly are.
fn push_char(bytes: &mut Vec<u8>, c: char) {
    match u8::try_from(u32::from(c)) {
        Ok(byte) => bytes.push(byte),
        Err(_) => bytes.extend(c.to_string().as_bytes()),
    }
}

// ---------- What a template says ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Order {
    Little,
    #[default]
    Big,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Num {
    Unsigned,
    Signed,
    Float,
    Char,
}

/// A number or character as stored: what kind, how many bytes, and a byte order if the type gives its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Prim {
    num: Num,
    size: u8,
    order: Option<Order>,
}

fn prim_named(name: &str) -> Option<Prim> {
    let (base, order) = if let Some(base) = name.strip_suffix("le") {
        (base, Some(Order::Little))
    } else if let Some(base) = name.strip_suffix("be") {
        (base, Some(Order::Big))
    } else {
        (name, None)
    };
    let (num, size) = match base {
        "char" => (Num::Char, 1),
        "u8" => (Num::Unsigned, 1),
        "u16" => (Num::Unsigned, 2),
        "u24" => (Num::Unsigned, 3),
        "u32" => (Num::Unsigned, 4),
        "u64" => (Num::Unsigned, 8),
        "s8" | "i8" => (Num::Signed, 1),
        "s16" | "i16" => (Num::Signed, 2),
        "s24" | "i24" => (Num::Signed, 3),
        "s32" | "i32" => (Num::Signed, 4),
        "s64" | "i64" => (Num::Signed, 8),
        "f32" => (Num::Float, 4),
        "f64" => (Num::Float, 8),
        _ => return None,
    };
    if order.is_some() && size == 1 {
        return None;
    }
    Some(Prim { num, size, order })
}

/// How a number is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Format {
    Plain,
    Hex,
    Dec,
    Bin,
    Oct,
    Char,
    DosDate,
    DosTime,
    Unix,
    /// Days since 1 January 1978, as AmigaDOS keeps dates.
    AmigaDays,
    /// An enum or flags type, by name.
    Named(String),
}

fn format_named(name: &str) -> Format {
    match name {
        "hex" => Format::Hex,
        "dec" => Format::Dec,
        "bin" => Format::Bin,
        "oct" => Format::Oct,
        "char" => Format::Char,
        "dosdate" => Format::DosDate,
        "dostime" => Format::DosTime,
        "unix" => Format::Unix,
        "amigadays" => Format::AmigaDays,
        other => Format::Named(other.to_string()),
    }
}

#[derive(Clone, Debug)]
struct EnumDef {
    base: Prim,
    flags: bool,
    entries: Vec<(String, i128, Option<String>)>,
}

#[derive(Clone, Debug)]
enum Expr {
    Int(i128),
    Float(f64),
    Str(Vec<u8>),
    Name(String),
    Member(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Debug)]
enum Count {
    One,
    Fixed(Expr),
    /// As many as there's room for, to the end of the file.
    ToEnd,
    While(Expr),
}

#[derive(Clone, Debug)]
struct Field {
    ty: String,
    name: String,
    count: Count,
    at: Option<Expr>,
    format: Option<Format>,
    note: Option<String>,
}

#[derive(Clone, Debug)]
enum Kind {
    Field(Field),
    If(Vec<(Expr, Vec<Stmt>)>, Option<Vec<Stmt>>),
    Let(String, Expr),
    Calc { name: String, format: Option<Format>, expr: Expr, note: Option<String> },
    Order(Order),
    Goto(Expr),
    Skip(Expr),
    Check(Expr, String),
    Stop(Option<String>),
    Label(Expr),
}

#[derive(Clone, Debug)]
struct Stmt {
    kind: Kind,
    line: usize,
}

/// A template, read and checked: what it's called, the files it's for, and how they're laid out.
#[derive(Clone, Debug)]
pub struct Template {
    pub name: String,
    /// File endings, in lower case and without the dot. A template with none is chosen by its `magic` alone.
    pub endings: Vec<String>,
    magic: Option<Expr>,
    magic_line: usize,
    order: Order,
    enums: HashMap<String, EnumDef>,
    structs: HashMap<String, Vec<Stmt>>,
    functions: HashMap<String, (Vec<String>, Expr, usize)>,
    /// Every enum and flags entry by its own name, with its value.
    constants: HashMap<String, i128>,
    body: Vec<Stmt>,
}

struct Parser {
    toks: Vec<Token>,
    at: usize,
}

impl Parser {
    fn token(&self) -> &Token {
        &self.toks[self.at.min(self.toks.len() - 1)]
    }

    fn peek(&self) -> &Tok {
        &self.token().tok
    }

    fn line(&self) -> usize {
        self.token().line
    }

    /// The next word, moving past it. Past the end there's only the end, so stepping back always works.
    fn next(&mut self) -> Tok {
        let tok = self.token().tok.clone();
        self.at += 1;
        tok
    }

    fn is(&self, sym: &str) -> bool {
        matches!(self.peek(), Tok::Sym(s) if *s == sym)
    }

    fn is_word(&self, word: &str) -> bool {
        matches!(self.peek(), Tok::Ident(w) if w == word)
    }

    fn eat(&mut self, sym: &str) -> bool {
        let found = self.is(sym);
        if found {
            self.next();
        }
        found
    }

    fn expect(&mut self, sym: &str) -> Result<(), Problem> {
        if self.eat(sym) { Ok(()) } else { Err(self.wanted(&format!("'{sym}'"))) }
    }

    /// What was found where something else was wanted, in words.
    fn wanted(&self, what: &str) -> Problem {
        let found = match self.peek() {
            Tok::Ident(w) => format!("'{w}'"),
            Tok::Int(n) => n.to_string(),
            Tok::Float(f) => f.to_string(),
            Tok::Str(_) => crate::i18n::tr("some text").to_string(),
            Tok::Sym(s) => format!("'{s}'"),
            Tok::Newline => crate::i18n::tr("the end of the line").to_string(),
            Tok::End => crate::i18n::tr("the end of the template").to_string(),
        };
        problem(self.line(), crate::i18n::tf!("{what} was expected here, not {found}."))
    }

    fn ident(&mut self, what: &str) -> Result<String, Problem> {
        match self.peek().clone() {
            Tok::Ident(word) => {
                self.next();
                Ok(word)
            }
            _ => Err(self.wanted(what)),
        }
    }

    fn text(&mut self) -> Result<String, Problem> {
        self.optional_text().ok_or_else(|| self.wanted(crate::i18n::tr("Some text in quotes")))
    }

    fn optional_text(&mut self) -> Option<String> {
        if let Tok::Str(bytes) = self.peek().clone() {
            self.next();
            Some(latin1(&bytes))
        } else {
            None
        }
    }

    fn skip_lines(&mut self) {
        while matches!(self.peek(), Tok::Newline) || self.is(";") {
            self.next();
        }
    }

    /// The end of a statement: a new line, a semicolon, or the brace closing its block.
    fn end_statement(&mut self) -> Result<(), Problem> {
        if matches!(self.peek(), Tok::Newline | Tok::End) || self.is(";") {
            self.next();
            Ok(())
        } else if self.is("}") {
            Ok(())
        } else {
            Err(self.wanted(crate::i18n::tr("The end of the line")))
        }
    }

    fn block(&mut self) -> Result<Vec<Stmt>, Problem> {
        self.expect("{")?;
        let mut stmts = Vec::new();
        loop {
            self.skip_lines();
            if self.eat("}") {
                return Ok(stmts);
            }
            if matches!(self.peek(), Tok::End) {
                return Err(self.wanted("'}'"));
            }
            stmts.push(self.statement()?);
        }
    }

    fn statement(&mut self) -> Result<Stmt, Problem> {
        let line = self.line();
        let word = self.ident(crate::i18n::tr("A field's type, or a word such as if or let"))?;
        let kind = match word.as_str() {
            "if" => {
                let mut arms = vec![(self.expr()?, self.block()?)];
                let mut otherwise = None;
                loop {
                    // `else` may start the next line, after the closing brace.
                    let back = self.at;
                    self.skip_lines();
                    if !self.is_word("else") {
                        self.at = back;
                        break;
                    }
                    self.next();
                    if self.is_word("if") {
                        self.next();
                        arms.push((self.expr()?, self.block()?));
                    } else {
                        otherwise = Some(self.block()?);
                        break;
                    }
                }
                Kind::If(arms, otherwise)
            }
            "let" => {
                let name = self.ident(crate::i18n::tr("A name"))?;
                self.expect("=")?;
                Kind::Let(name, self.expr()?)
            }
            "calc" => {
                let name = self.ident(crate::i18n::tr("A name"))?;
                let format = if self.eat(":") { Some(format_named(&self.ident(crate::i18n::tr("A way of showing it, such as hex"))?)) } else { None };
                self.expect("=")?;
                let expr = self.expr()?;
                Kind::Calc { name, format, expr, note: self.optional_text() }
            }
            "order" => Kind::Order(self.order()?),
            "goto" => Kind::Goto(self.expr()?),
            "skip" => Kind::Skip(self.expr()?),
            "check" => {
                let expr = self.expr()?;
                Kind::Check(expr, self.text()?)
            }
            "stop" => Kind::Stop(self.optional_text()),
            "label" => Kind::Label(self.expr()?),
            "struct" | "enum" | "flags" | "def" | "name" | "endings" | "magic" => {
                return Err(problem(line, crate::i18n::tf!("'{word}' can only be used outside a struct, at the start of a line.")));
            }
            _ => Kind::Field(self.field(word)?),
        };
        self.end_statement()?;
        Ok(Stmt { kind, line })
    }

    fn order(&mut self) -> Result<Order, Problem> {
        match self.ident(crate::i18n::tr("big or little"))?.as_str() {
            "big" => Ok(Order::Big),
            "little" => Ok(Order::Little),
            _ => {
                self.at -= 1;
                Err(self.wanted(crate::i18n::tr("big or little")))
            }
        }
    }

    fn field(&mut self, ty: String) -> Result<Field, Problem> {
        let name = self.ident(crate::i18n::tr("The field's name"))?;
        let count = if self.eat("[") {
            let count = if self.is("]") {
                Count::ToEnd
            } else if self.is_word("while") {
                self.next();
                Count::While(self.expr()?)
            } else {
                Count::Fixed(self.expr()?)
            };
            self.expect("]")?;
            count
        } else {
            Count::One
        };
        let at = if self.eat("@") { Some(self.expr()?) } else { None };
        let format = if self.eat(":") { Some(format_named(&self.ident(crate::i18n::tr("A way of showing it, such as hex"))?)) } else { None };
        let note = self.optional_text();
        Ok(Field { ty, name, count, at, format, note })
    }

    // Expressions, from the loosest binding to the tightest.

    fn expr(&mut self) -> Result<Expr, Problem> {
        let test = self.binary(0)?;
        if self.eat("?") {
            let yes = self.expr()?;
            self.expect(":")?;
            let no = self.expr()?;
            return Ok(Expr::Cond(Box::new(test), Box::new(yes), Box::new(no)));
        }
        Ok(test)
    }

    fn binary(&mut self, level: usize) -> Result<Expr, Problem> {
                // As Rust has them, not C: `a & mask == 0` tests the bits, rather than ANDing with a comparison.
        const LEVELS: [&[&str]; 9] = [&["||"], &["&&"], &["==", "!=", "<", "<=", ">", ">="], &["|"], &["^"], &["&"], &["<<", ">>"], &["+", "-"], &["*", "/", "%"]];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        while let Tok::Sym(sym) = self.peek().clone() {
            if !LEVELS[level].contains(&sym) {
                break;
            }
            self.next();
            let right = self.binary(level + 1)?;
            left = Expr::Binary(sym, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, Problem> {
        for sym in ["-", "!", "~"] {
            if self.eat(sym) {
                return Ok(Expr::Unary(sym, Box::new(self.unary()?)));
            }
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, Problem> {
        let mut expr = self.primary()?;
        loop {
            if self.eat(".") {
                expr = Expr::Member(Box::new(expr), self.ident(crate::i18n::tr("A field's name"))?);
            } else if self.eat("[") {
                let index = self.expr()?;
                self.expect("]")?;
                expr = Expr::Index(Box::new(expr), Box::new(index));
            } else {
                return Ok(expr);
            }
        }
    }

    fn primary(&mut self) -> Result<Expr, Problem> {
        match self.next() {
            Tok::Int(n) => Ok(Expr::Int(n)),
            Tok::Float(f) => Ok(Expr::Float(f)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::Ident(word) => {
                if self.eat("(") {
                    let mut args = Vec::new();
                    if !self.eat(")") {
                        loop {
                            args.push(self.expr()?);
                            if self.eat(")") {
                                break;
                            }
                            self.expect(",")?;
                        }
                    }
                    Ok(Expr::Call(word, args))
                } else {
                    Ok(Expr::Name(word))
                }
            }
            Tok::Sym("(") => {
                let inner = self.expr()?;
                self.expect(")")?;
                Ok(inner)
            }
            _ => {
                self.at -= 1;
                Err(self.wanted(crate::i18n::tr("A number, a name or a bracket")))
            }
        }
    }

    fn enum_body(&mut self, flags: bool) -> Result<(String, EnumDef), Problem> {
        let name = self.ident(crate::i18n::tr("The type's name"))?;
        self.expect(":")?;
        let base_line = self.line();
        let base_name = self.ident(crate::i18n::tr("The type it's stored as, such as u8"))?;
        let base = prim_named(&base_name).filter(|p| matches!(p.num, Num::Unsigned | Num::Signed)).ok_or_else(|| problem(base_line, crate::i18n::tf!("An enum or flags type is stored as a whole number, such as u8 or u16be, not {base_name}.")))?;
        self.skip_lines();
        self.expect("{")?;
        let mut entries = Vec::new();
        loop {
            self.skip_lines();
            if self.eat("}") {
                break;
            }
            let entry = self.ident(crate::i18n::tr("An entry's name"))?;
            self.expect("=")?;
            let value_line = self.line();
            let Expr::Int(value) = fold(self.expr()?).map_err(|message| problem(value_line, message))? else {
                return Err(problem(value_line, crate::i18n::tr("An entry's value must be a number.").into()));
            };
            let note = self.optional_text();
            entries.push((entry, value, note));
            self.end_statement()?;
        }
        Ok((name, EnumDef { base, flags, entries }))
    }
}

/// An enum entry's value worked out where it's written, so `1 << 3` can be used for flags.
fn fold(expr: Expr) -> Result<Expr, String> {
    match expr {
        Expr::Int(_) => Ok(expr),
        Expr::Unary("-", inner) => match fold(*inner)? {
            Expr::Int(n) => Ok(Expr::Int(-n)),
            other => Ok(other),
        },
        Expr::Unary("~", inner) => match fold(*inner)? {
            Expr::Int(n) => Ok(Expr::Int(!n)),
            other => Ok(other),
        },
        Expr::Binary(op, a, b) => match (fold(*a)?, fold(*b)?) {
            (Expr::Int(a), Expr::Int(b)) => int_op(op, a, b).map(Expr::Int),
            _ => Ok(Expr::Str(Vec::new())),
        },
        _ => Ok(Expr::Str(Vec::new())),
    }
}

impl Template {
    /// Reads a template's text, or says on which line it went wrong and why.
    pub fn parse(text: &str) -> Result<Template, Problem> {
        let mut p = Parser { toks: lex(text)?, at: 0 };
        let mut t = Template {
            name: String::new(),
            endings: Vec::new(),
            magic: None,
            magic_line: 0,
            order: Order::Big,
            enums: HashMap::new(),
            structs: HashMap::new(),
            functions: HashMap::new(),
            constants: HashMap::new(),
            body: Vec::new(),
        };
        loop {
            p.skip_lines();
            let line = p.line();
            match p.peek().clone() {
                Tok::End => break,
                Tok::Ident(word) => match word.as_str() {
                    "name" if matches!(p.toks.get(p.at + 1).map(|t| &t.tok), Some(Tok::Str(_))) => {
                        p.next();
                        t.name = p.text()?;
                        p.end_statement()?;
                    }
                    "endings" => {
                        p.next();
                        while !matches!(p.peek(), Tok::Newline | Tok::End) && !p.is(";") {
                            match p.next() {
                                Tok::Ident(w) => t.endings.push(w.to_lowercase()),
                                Tok::Str(s) => t.endings.push(latin1(&s).trim_start_matches('.').to_lowercase()),
                                Tok::Sym(",") => {}
                                _ => {
                                    p.at -= 1;
                                    return Err(p.wanted(crate::i18n::tr("A file ending")));
                                }
                            }
                        }
                        p.end_statement()?;
                    }
                    "magic" => {
                        p.next();
                        t.magic_line = line;
                        t.magic = Some(p.expr()?);
                        p.end_statement()?;
                    }
                    "order" if t.body.is_empty() && t.structs.is_empty() => {
                        p.next();
                        t.order = p.order()?;
                        p.end_statement()?;
                    }
                    "struct" => {
                        p.next();
                        let name = p.ident(crate::i18n::tr("The struct's name"))?;
                        if prim_named(&name).is_some() || t.structs.contains_key(&name) || t.enums.contains_key(&name) {
                            return Err(problem(line, crate::i18n::tf!("There's already a type called {name}.")));
                        }
                        p.skip_lines();
                        let body = p.block()?;
                        t.structs.insert(name, body);
                        p.end_statement()?;
                    }
                    "enum" | "flags" => {
                        p.next();
                        let (name, def) = p.enum_body(word == "flags")?;
                        if prim_named(&name).is_some() || t.structs.contains_key(&name) || t.enums.contains_key(&name) {
                            return Err(problem(line, crate::i18n::tf!("There's already a type called {name}.")));
                        }
                        for (entry, value, _) in &def.entries {
                            t.constants.entry(entry.clone()).or_insert(*value);
                        }
                        t.enums.insert(name, def);
                        p.end_statement()?;
                    }
                    "def" => {
                        p.next();
                        let name = p.ident(crate::i18n::tr("The function's name"))?;
                        p.expect("(")?;
                        let mut params = Vec::new();
                        if !p.eat(")") {
                            loop {
                                params.push(p.ident(crate::i18n::tr("A name"))?);
                                if p.eat(")") {
                                    break;
                                }
                                p.expect(",")?;
                            }
                        }
                        p.expect("=")?;
                        let body = p.expr()?;
                        t.functions.insert(name, (params, body, line));
                        p.end_statement()?;
                    }
                    _ => {
                        let stmt = p.statement()?;
                        t.body.push(stmt);
                    }
                },
                _ => return Err(p.wanted(crate::i18n::tr("A field's type, or a word such as struct or if"))),
            }
        }
        if t.name.is_empty() {
            return Err(problem(1, crate::i18n::tr("A template needs a name: a line such as name \"IFF picture\" at the top.").into()));
        }
        t.check_types()?;
        Ok(t)
    }

    /// Every type and way of showing named in the template is one it has, found before any file is read.
    fn check_types(&self) -> Result<(), Problem> {
        fn walk(t: &Template, stmts: &[Stmt]) -> Result<(), Problem> {
            for stmt in stmts {
                match &stmt.kind {
                    Kind::Field(f) => {
                        let known = prim_named(&f.ty).is_some() || matches!(f.ty.as_str(), "cstring" | "pstring") || t.enums.contains_key(&f.ty) || t.structs.contains_key(&f.ty);
                        if !known {
                            return Err(problem(stmt.line, crate::i18n::tf!("There's no type called {}.", f.ty)));
                        }
                        t.check_format(f.format.as_ref(), stmt.line)?;
                    }
                    Kind::Calc { format, .. } => t.check_format(format.as_ref(), stmt.line)?,
                    Kind::If(arms, otherwise) => {
                        for (_, body) in arms {
                            walk(t, body)?;
                        }
                        if let Some(body) = otherwise {
                            walk(t, body)?;
                        }
                    }
                    _ => {}
                }
            }
            Ok(())
        }
        walk(self, &self.body)?;
        let mut names: Vec<&String> = self.structs.keys().collect();
        names.sort();
        for name in names {
            walk(self, &self.structs[name])?;
        }
        Ok(())
    }

    fn check_format(&self, format: Option<&Format>, line: usize) -> Result<(), Problem> {
        match format {
            Some(Format::Named(name)) if !self.enums.contains_key(name) => Err(problem(line, crate::i18n::tf!("{name} isn't a way of showing a value: use hex, dec, bin, oct, char, dosdate, dostime, unix, amigadays, or an enum or flags type."))),
            _ => Ok(()),
        }
    }

    /// Whether this template is for this file: its ending is one listed (when any are), and its magic holds.
    pub fn fits(&self, source: &dyn Source, ending: &str) -> bool {
        if !self.endings.is_empty() && !self.endings.iter().any(|e| e.eq_ignore_ascii_case(ending)) {
            return false;
        }
        let Some(magic) = &self.magic else {
            // A template with neither is never chosen by itself.
            return !self.endings.is_empty();
        };
        let mut run = Run::new(self, source);
        run.eval(magic, self.magic_line).is_ok_and(|v| v.truthy())
    }

    /// Reads a file with this template. The tree holds what was read up to any problem, and the problem.
    pub fn read(&self, source: &dyn Source) -> Tree {
        let mut run = Run::new(self, source);
        let root = Node { name: self.name.clone(), type_name: String::new(), start: 0, end: 0, shown: String::new(), note: String::new(), kind: NodeKind::Struct, children: Vec::new(), parent: None, depth: 0, colour: 0, element: None, line: 0 };
        run.nodes.push(root);
        run.values.push(Value::Node(0));
        run.scopes.push(Scope { vars: HashMap::new(), nodes: HashMap::new(), order: self.order });
        let outcome = run.exec(&self.body, 0);
        run.nodes[0].end = run.here.max(run.nodes[0].children.iter().map(|c| run.nodes[*c].end).max().unwrap_or(0));
        let error = match outcome {
            Err(Flow::Problem(p)) => Some(p),
            Ok(()) | Err(Flow::Stop) => None,
        };
        let mut marks: Vec<(u64, u64, usize)> = run.nodes.iter().enumerate().filter(|(_, n)| n.kind == NodeKind::Field && n.end > n.start).map(|(i, n)| (n.start, n.end, i)).collect();
        marks.sort_by_key(|m| (m.0, std::cmp::Reverse(m.1)));
        Tree { nodes: run.nodes, error, marks, enums: self.enums.clone() }
    }
}

// ---------- The tree a file is read into ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// Bytes of the file read as a value.
    Field,
    Struct,
    /// A list of structs, each a node of its own.
    Array,
    /// A value worked out from others, with no bytes of its own.
    Calc,
    /// A check that didn't hold.
    Warning,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub type_name: String,
    pub start: u64,
    pub end: u64,
    /// The value, as shown.
    pub shown: String,
    /// What the template says about the field.
    pub note: String,
    pub kind: NodeKind,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
    pub depth: u16,
    /// Which of the field colours it's shown in.
    pub colour: u8,
    /// For a list of numbers held as one field: how each is stored, so one of them can be named and shown.
    element: Option<(Prim, Order, Format)>,
    /// The template line it came from.
    pub line: usize,
}

/// A file read with a template: every field (the first node is the whole file), any problem met, and the fields
/// by where their bytes are, for the hex view.
#[derive(Clone, Debug)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub error: Option<Problem>,
    marks: Vec<(u64, u64, usize)>,
    enums: HashMap<String, EnumDef>,
}

impl Tree {
    /// The innermost field whose bytes include this offset.
    pub fn field_at(&self, offset: u64) -> Option<usize> {
        let after = self.marks.partition_point(|m| m.0 <= offset);
        self.marks[..after].iter().rev().take(16).find(|m| m.1 > offset).map(|m| m.2)
    }

    /// A field's name with the names of the structs it's in: `chunks[2].width`.
    pub fn path(&self, node: usize) -> String {
        let mut parts = Vec::new();
        let mut at = Some(node);
        while let Some(i) = at.filter(|i| *i != 0) {
            parts.push(self.nodes[i].name.as_str());
            at = self.nodes[i].parent;
        }
        let mut out = String::new();
        for part in parts.iter().rev() {
            if !out.is_empty() && !part.starts_with('[') {
                out.push('.');
            }
            out.push_str(part);
        }
        out
    }

    /// What the hex view says about the byte under the pointer: the field's name and value, and for a list of
    /// numbers held as one field, which of them it is.
    pub fn describe(&self, offset: u64, source: &dyn Source) -> Option<String> {
        let i = self.field_at(offset)?;
        let node = &self.nodes[i];
        let mut words = format!("{}: {}", self.path(i), node.shown);
        if let Some((prim, order, format)) = &node.element {
            let size = u64::from(prim.size);
            let index = (offset - node.start) / size;
            let bytes = source.read(node.start + index * size, prim.size as usize);
            if bytes.len() == prim.size as usize {
                let value = decode(&bytes, *prim, *order);
                words = format!("{}[{index}]: {}", self.path(i), show_value(&value, format, *prim, &self.enums));
            }
        }
        if !node.note.is_empty() {
            words.push('\n');
            words.push_str(&node.note);
        }
        Some(words)
    }
}

// ---------- Reading a file ----------

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Int(i128),
    Float(f64),
    Str(Vec<u8>),
    Node(usize),
}

impl Value {
    fn truthy(&self) -> bool {
        match self {
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Node(_) => true,
        }
    }
}

enum Flow {
    Problem(Problem),
    /// `stop`: the rest isn't read, and that's not a mistake.
    Stop,
}

impl From<Problem> for Flow {
    fn from(p: Problem) -> Self {
        Flow::Problem(p)
    }
}

struct Scope {
    vars: HashMap<String, Value>,
    /// The node each field read in this scope became, for functions such as `start_of`.
    nodes: HashMap<String, usize>,
    order: Order,
}

struct Run<'a> {
    t: &'a Template,
    src: &'a dyn Source,
    len: u64,
    here: u64,
    nodes: Vec<Node>,
    values: Vec<Value>,
    scopes: Vec<Scope>,
    /// The element number of each list being read, innermost last, for `index`.
    indices: Vec<i128>,
    fields: usize,
    steps: u64,
    calls: usize,
}

/// The ways a template's type can be stored.
enum Ty<'t> {
    Prim(Prim, Format),
    CString,
    PString,
    Struct(&'t str, &'t [Stmt]),
}

impl<'a> Run<'a> {
    fn new(t: &'a Template, src: &'a dyn Source) -> Self {
        Run { t, src, len: src.len(), here: 0, nodes: Vec::new(), values: Vec::new(), scopes: vec![Scope { vars: HashMap::new(), nodes: HashMap::new(), order: t.order }], indices: Vec::new(), fields: 0, steps: 0, calls: 0 }
    }

    fn order(&self) -> Order {
        self.scopes.last().map_or(self.t.order, |s| s.order)
    }

    fn step(&mut self, line: usize) -> Result<(), Problem> {
        self.steps += 1;
        if self.steps > MOST_STEPS {
            return Err(problem(line, crate::i18n::tr("The template went round its lists too many times; it may never stop.").into()));
        }
        Ok(())
    }

    fn add(&mut self, parent: usize, mut node: Node, value: Value, line: usize) -> Result<usize, Problem> {
        if self.nodes.len() >= MOST_NODES {
            return Err(problem(line, crate::i18n::tf!("The file was read into more than {MOST_NODES} fields, so only the first are shown.")));
        }
        let index = self.nodes.len();
        node.parent = Some(parent);
        node.depth = self.nodes[parent].depth + 1;
        node.line = line;
        if node.kind == NodeKind::Field {
            node.colour = (self.fields % COLOURS as usize) as u8;
            self.fields += 1;
        }
        self.nodes.push(node);
        self.values.push(value);
        self.nodes[parent].children.push(index);
        Ok(index)
    }

    fn bind(&mut self, name: &str, value: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.vars.insert(name.to_string(), value);
        }
    }

    fn exec(&mut self, stmts: &'a [Stmt], parent: usize) -> Result<(), Flow> {
        for stmt in stmts {
            let line = stmt.line;
            match &stmt.kind {
                Kind::Field(f) => self.field(f, parent, line)?,
                Kind::If(arms, otherwise) => {
                    let mut taken = false;
                    for (test, body) in arms {
                        if self.eval(test, line)?.truthy() {
                            self.exec(body, parent)?;
                            taken = true;
                            break;
                        }
                    }
                    if !taken && let Some(body) = otherwise {
                        self.exec(body, parent)?;
                    }
                }
                Kind::Let(name, expr) => {
                    let value = self.eval(expr, line)?;
                    self.bind(name, value);
                }
                Kind::Calc { name, format, expr, note } => {
                    let value = self.eval(expr, line)?;
                    let shown = match (&value, format) {
                        (Value::Int(n), Some(format)) => {
                            // Shown as wide as the number needs, as if it were stored in the fewest bytes it fits.
                            let size = [1u8, 2, 4, 8].into_iter().find(|size| (*n as u128) < 1u128 << (*size as u32 * 8)).unwrap_or(8);
                            show_value(&value, format, Prim { num: Num::Unsigned, size, order: None }, &self.t.enums)
                        }
                        _ => show_value(&value, &Format::Plain, Prim { num: Num::Signed, size: 8, order: None }, &self.t.enums),
                    };
                    let node = leaf(name, "", self.here, self.here, shown, note, NodeKind::Calc);
                    let index = self.add(parent, node, value.clone(), line)?;
                    self.bind(name, value);
                    if let Some(scope) = self.scopes.last_mut() {
                        scope.nodes.insert(name.clone(), index);
                    }
                }
                Kind::Order(order) => {
                    if let Some(scope) = self.scopes.last_mut() {
                        scope.order = *order;
                    }
                }
                Kind::Goto(expr) => self.here = self.offset(expr, line)?,
                Kind::Skip(expr) => {
                    let by = self.int(expr, line)?;
                    self.here = u64::try_from(i128::from(self.here) + by).map_err(|_| problem(line, crate::i18n::tr("That would go back before the start of the file.").into()))?;
                }
                Kind::Check(expr, message) => {
                    if !self.eval(expr, line)?.truthy() {
                        let node = leaf("", "", self.here, self.here, message.clone(), &None, NodeKind::Warning);
                        self.add(parent, node, Value::Int(0), line)?;
                    }
                }
                Kind::Stop(message) => {
                    if let Some(message) = message {
                        let node = leaf("", "", self.here, self.here, message.clone(), &None, NodeKind::Warning);
                        self.add(parent, node, Value::Int(0), line)?;
                    }
                    return Err(Flow::Stop);
                }
                Kind::Label(expr) => {
                    // A field named is shown as it's shown in the tree, enum names and all.
                    let named = matches!(expr, Expr::Name(_) | Expr::Member(..) | Expr::Index(..)).then(|| self.node_of(expr, line).ok()).flatten().filter(|n| matches!(self.nodes[*n].kind, NodeKind::Field | NodeKind::Calc));
                    self.nodes[parent].shown = match named {
                        Some(node) => self.nodes[node].shown.clone(),
                        None => {
                            let value = self.eval(expr, line)?;
                            show_value(&value, &Format::Plain, Prim { num: Num::Signed, size: 8, order: None }, &self.t.enums)
                        }
                    };
                }
            }
        }
        Ok(())
    }

    fn resolve(&self, f: &'a Field, line: usize) -> Result<Ty<'a>, Problem> {
        if let Some(prim) = prim_named(&f.ty) {
            let format = f.format.clone().unwrap_or(Format::Plain);
            return Ok(Ty::Prim(prim, format));
        }
        match f.ty.as_str() {
            "cstring" => return Ok(Ty::CString),
            "pstring" => return Ok(Ty::PString),
            _ => {}
        }
        if let Some(def) = self.t.enums.get(&f.ty) {
            return Ok(Ty::Prim(def.base, f.format.clone().unwrap_or_else(|| Format::Named(f.ty.clone()))));
        }
        if let Some((name, body)) = self.t.structs.get_key_value(&f.ty) {
            return Ok(Ty::Struct(name, body));
        }
        Err(problem(line, crate::i18n::tf!("There's no type called {}.", f.ty)))
    }

    fn past_end(&self, name: &str, line: usize, at: u64) -> Problem {
        problem(line, crate::i18n::tf!("{name} would run past the end of the file (at offset {at}, 0x{at:X}).", name = self.named(name)))
    }

    fn named(&self, name: &str) -> String {
        format!("'{name}'")
    }

    fn field(&mut self, f: &'a Field, parent: usize, line: usize) -> Result<(), Flow> {
        let saved = self.here;
        if let Some(at) = &f.at {
            self.here = self.offset(at, line)?;
        }
        let ty = self.resolve(f, line)?;
        // The first node made while reading a field is always the field's own.
        let first = self.nodes.len();
        let value = match &f.count {
            Count::One => self.one(&ty, &f.name, f, parent, line)?,
            count => self.array(&ty, count, f, parent, line)?,
        };
        self.bind(&f.name, value);
        if first < self.nodes.len()
            && let Some(scope) = self.scopes.last_mut()
        {
            scope.nodes.insert(f.name.clone(), first);
        }
        if f.at.is_some() {
            self.here = saved;
        }
        Ok(())
    }

    /// One field of this type at `here`, which moves past it.
    fn one(&mut self, ty: &Ty<'a>, name: &str, f: &Field, parent: usize, line: usize) -> Result<Value, Flow> {
        let start = self.here;
        match ty {
            Ty::Prim(prim, format) => {
                let bytes = self.src.read(start, prim.size as usize);
                if bytes.len() < prim.size as usize {
                    return Err(self.past_end(name, line, start).into());
                }
                let order = prim.order.unwrap_or(self.order());
                let mut value = decode(&bytes, *prim, order);
                if prim.num == Num::Char && let Value::Int(n) = value {
                    value = Value::Str(if n == 0 { Vec::new() } else { vec![n as u8] });
                }
                let shown = show_value(&value, format, *prim, &self.t.enums);
                self.here = start + u64::from(prim.size);
                let mut node = leaf(name, &f.ty, start, self.here, shown, &f.note, NodeKind::Field);
                if node.note.is_empty() {
                    node.note = self.entry_note(format, &value).unwrap_or_default();
                }
                self.add(parent, node, value.clone(), line)?;
                Ok(value)
            }
            Ty::CString | Ty::PString => {
                let (text, length) = if matches!(ty, Ty::CString) {
                    let mut text = Vec::new();
                    let mut at = start;
                    loop {
                        let chunk = self.src.read(at, 256);
                        if chunk.is_empty() {
                            return Err(self.past_end(name, line, at).into());
                        }
                        if let Some(end) = chunk.iter().position(|b| *b == 0) {
                            text.extend_from_slice(&chunk[..end]);
                            break;
                        }
                        text.extend_from_slice(&chunk);
                        at += chunk.len() as u64;
                        if text.len() > MOST_STRING {
                            return Err(problem(line, crate::i18n::tf!("{} has no zero byte to end it in the first {MOST_STRING} bytes.", self.named(name))).into());
                        }
                    }
                    let length = text.len() as u64 + 1;
                    (text, length)
                } else {
                    let count = *self.src.read(start, 1).first().ok_or_else(|| self.past_end(name, line, start))? as usize;
                    let text = self.src.read(start + 1, count);
                    if text.len() < count {
                        return Err(self.past_end(name, line, start).into());
                    }
                    (text, count as u64 + 1)
                };
                self.here = start + length;
                let node = leaf(name, &f.ty, start, self.here, show_text(&text), &f.note, NodeKind::Field);
                self.add(parent, node, Value::Str(text.clone()), line)?;
                Ok(Value::Str(text))
            }
            Ty::Struct(type_name, body) => {
                if self.nodes[parent].depth as usize >= MOST_DEPTH {
                    return Err(problem(line, crate::i18n::tf!("Structs go more than {MOST_DEPTH} deep here; a struct may be inside itself for ever.")).into());
                }
                let node = leaf(name, type_name, start, start, String::new(), &f.note, NodeKind::Struct);
                let index = self.add(parent, node, Value::Int(0), line)?;
                self.values[index] = Value::Node(index);
                let order = self.order();
                self.scopes.push(Scope { vars: HashMap::new(), nodes: HashMap::new(), order });
                let outcome = self.exec(body, index);
                self.scopes.pop();
                self.nodes[index].end = self.here.max(start);
                outcome?;
                Ok(Value::Node(index))
            }
        }
    }

    fn array(&mut self, ty: &Ty<'a>, count: &Count, f: &Field, parent: usize, line: usize) -> Result<Value, Flow> {
        let start = self.here;
        let fixed = match count {
            Count::Fixed(expr) => {
                let n = self.int(expr, line)?;
                Some(u64::try_from(n).map_err(|_| problem(line, crate::i18n::tf!("{} can't have {n} entries.", self.named(&f.name))))?)
            }
            _ => None,
        };
        if let Ty::Prim(prim, format) = ty {
            let size = u64::from(prim.size);
            let n = match count {
                Count::Fixed(_) => fixed.unwrap_or(0),
                Count::ToEnd => self.len.saturating_sub(start) / size,
                Count::While(test) => {
                    let mut n = 0u64;
                    loop {
                        self.step(line)?;
                        self.indices.push(n as i128);
                        let saved = self.here;
                        self.here = start + n * size;
                        let going = self.eval(test, line);
                        self.here = saved;
                        self.indices.pop();
                        if !going?.truthy() || start + (n + 1) * size > self.len {
                            break;
                        }
                        n += 1;
                    }
                    n
                }
                Count::One => 1,
            };
            let end = n.checked_mul(size).and_then(|bytes| start.checked_add(bytes)).filter(|end| *end <= self.len).ok_or_else(|| self.past_end(&f.name, line, start))?;
            let order = prim.order.unwrap_or(self.order());
            let (shown, value) = self.show_list(start, n, *prim, order, format);
            self.here = end;
            let mut node = leaf(&f.name, &format!("{}[{n}]", f.ty), start, end, shown, &f.note, NodeKind::Field);
            if prim.num != Num::Char {
                node.element = Some((*prim, order, format.clone()));
            }
            let index = self.add(parent, node, value, line)?;
            if prim.num != Num::Char {
                self.values[index] = Value::Node(index);
            }
            return Ok(self.values[index].clone());
        }
        let node = leaf(&f.name, &f.ty, start, start, String::new(), &f.note, NodeKind::Array);
        let index = self.add(parent, node, Value::Int(0), line)?;
        self.values[index] = Value::Node(index);
        let mut n = 0u64;
        let outcome = loop {
            match count {
                Count::Fixed(_) if n >= fixed.unwrap_or(0) => break Ok(()),
                Count::ToEnd if self.here >= self.len => break Ok(()),
                Count::While(test) => {
                    self.indices.push(n as i128);
                    let going = self.eval(test, line);
                    self.indices.pop();
                    match going {
                        Ok(v) if !v.truthy() => break Ok(()),
                        Err(e) => break Err(Flow::Problem(e)),
                        Ok(_) => {}
                    }
                }
                _ => {}
            }
            if let Err(e) = self.step(line) {
                break Err(e.into());
            }
            let before = self.here;
            self.indices.push(n as i128);
            let read = self.one(ty, &format!("[{n}]"), f, index, line);
            self.indices.pop();
            if let Err(e) = read {
                break Err(e);
            }
            n += 1;
            if self.here == before && !matches!(count, Count::Fixed(_)) {
                break Err(problem(line, crate::i18n::tf!("An entry of {} took up no bytes, so the list would never end.", self.named(&f.name))).into());
            }
        };
        self.nodes[index].end = self.here.max(start);
        self.nodes[index].shown = crate::i18n::tf!("{n} entries");
        self.nodes[index].type_name = format!("{}[{n}]", f.ty);
        outcome?;
        Ok(Value::Node(index))
    }

    /// How a list of numbers or characters is shown, and its value: the text for characters, or the list itself.
    fn show_list(&self, start: u64, n: u64, prim: Prim, order: Order, format: &Format) -> (String, Value) {
        const MOST_SHOWN: u64 = 16;
        if prim.num == Num::Char {
            let mut bytes = self.src.read(start, n.min(4096) as usize);
            if let Some(end) = bytes.iter().position(|b| *b == 0) {
                bytes.truncate(end);
            }
            return (show_text(&bytes), Value::Str(bytes));
        }
        let shown_count = n.min(MOST_SHOWN);
        let bytes = self.src.read(start, (shown_count * u64::from(prim.size)) as usize);
        let more = if n > shown_count { " \u{2026}" } else { "" };
        if prim.size == 1 && *format == Format::Plain && prim.num == Num::Unsigned {
            let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
            return (format!("{}{more}", hex.join(" ")), Value::Int(0));
        }
        // In a list, a plain number is shown once, without its hex beside it.
        let format = if *format == Format::Plain { &Format::Dec } else { format };
        let values: Vec<String> = bytes.chunks_exact(prim.size as usize).map(|chunk| show_value(&decode(chunk, prim, order), format, prim, &self.t.enums)).collect();
        (format!("[{}{more}]", values.join(", ")), Value::Int(0))
    }

    /// What an enum's entry says about itself, for a field holding it.
    fn entry_note(&self, format: &Format, value: &Value) -> Option<String> {
        let (Format::Named(name), Value::Int(n)) = (format, value) else {
            return None;
        };
        let def = self.t.enums.get(name).filter(|d| !d.flags)?;
        def.entries.iter().find(|(_, v, _)| v == n).and_then(|(_, _, note)| note.clone())
    }

    fn offset(&mut self, expr: &Expr, line: usize) -> Result<u64, Problem> {
        let n = self.int(expr, line)?;
        u64::try_from(n).map_err(|_| problem(line, crate::i18n::tf!("{n} isn't a place in the file.")))
    }

    fn int(&mut self, expr: &Expr, line: usize) -> Result<i128, Problem> {
        match self.eval(expr, line)? {
            Value::Int(n) => Ok(n),
            Value::Float(f) => Ok(f as i128),
            _ => Err(problem(line, crate::i18n::tr("A number was wanted here.").into())),
        }
    }

    fn lookup(&self, name: &str, line: usize) -> Result<Value, Problem> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.vars.get(name) {
                return Ok(value.clone());
            }
        }
        if let Some(value) = self.t.constants.get(name) {
            return Ok(Value::Int(*value));
        }
        match name {
            "here" => Ok(Value::Int(i128::from(self.here))),
            "file_size" => Ok(Value::Int(i128::from(self.len))),
            "index" => self.indices.last().map(|i| Value::Int(*i)).ok_or_else(|| problem(line, crate::i18n::tr("index is only known inside a list.").into())),
            "true" => Ok(Value::Int(1)),
            "false" => Ok(Value::Int(0)),
            _ => Err(problem(line, crate::i18n::tf!("Nothing called {name} has been read yet."))),
        }
    }

    fn child(&self, node: usize, name: &str, line: usize) -> Result<usize, Problem> {
        self.nodes[node].children.iter().copied().find(|c| self.nodes[*c].name == name).ok_or_else(|| problem(line, crate::i18n::tf!("{} has no field called {name}.", self.nodes[node].name)))
    }

    /// The node an expression names, for functions such as `start_of` that ask about a field rather than its value.
    fn node_of(&mut self, expr: &Expr, line: usize) -> Result<usize, Problem> {
        match expr {
            Expr::Name(name) => self.scopes.iter().rev().find_map(|s| s.nodes.get(name).copied()).ok_or_else(|| problem(line, crate::i18n::tf!("Nothing called {name} has been read yet."))),
            Expr::Member(base, name) => {
                let base = self.node_of(base, line)?;
                self.child(base, name, line)
            }
            Expr::Index(base, index) => {
                let base = self.node_of(base, line)?;
                let i = self.int(index, line)?;
                usize::try_from(i).ok().and_then(|i| self.nodes[base].children.get(i).copied()).ok_or_else(|| problem(line, crate::i18n::tf!("{} has no entry {i}.", self.nodes[base].name)))
            }
            _ => Err(problem(line, crate::i18n::tr("The name of a field was wanted here.").into())),
        }
    }

    fn read_at(&self, at: i128, prim: Prim, line: usize) -> Result<Value, Problem> {
        let at = u64::try_from(at).map_err(|_| problem(line, crate::i18n::tf!("{at} isn't a place in the file.")))?;
        let bytes = self.src.read(at, prim.size as usize);
        if bytes.len() < prim.size as usize {
            return Err(problem(line, crate::i18n::tf!("Offset {at} (0x{at:X}) is past the end of the file.")));
        }
        Ok(decode(&bytes, prim, prim.order.unwrap_or(self.order())))
    }

    fn bytes_at(&self, at: i128, count: i128, line: usize) -> Result<Vec<u8>, Problem> {
        let (Ok(at), Ok(count)) = (u64::try_from(at), usize::try_from(count)) else {
            return Err(problem(line, crate::i18n::tr("That isn't a place in the file.").into()));
        };
        let bytes = self.src.read(at, count);
        if bytes.len() < count {
            return Err(problem(line, crate::i18n::tf!("Offset {at} (0x{at:X}) is past the end of the file.")));
        }
        Ok(bytes)
    }

    fn eval(&mut self, expr: &Expr, line: usize) -> Result<Value, Problem> {
        match expr {
            Expr::Int(n) => Ok(Value::Int(*n)),
            Expr::Float(f) => Ok(Value::Float(*f)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Name(name) => self.lookup(name, line),
            Expr::Member(base, name) => {
                let Value::Node(node) = self.eval(base, line)? else {
                    return Err(problem(line, crate::i18n::tf!("Only a struct has fields, so .{name} can't be read here.")));
                };
                let child = self.child(node, name, line)?;
                Ok(self.values[child].clone())
            }
            Expr::Index(base, index) => {
                let base = self.eval(base, line)?;
                let i = self.int(index, line)?;
                match base {
                    Value::Node(node) => {
                        if let Some((prim, order, _)) = self.nodes[node].element {
                            let size = i128::from(prim.size);
                            let count = i128::from(self.nodes[node].end - self.nodes[node].start) / size;
                            if i < 0 || i >= count {
                                return Err(problem(line, crate::i18n::tf!("{} has no entry {i}.", self.nodes[node].name)));
                            }
                            let bytes = self.src.read(self.nodes[node].start + (i * size) as u64, prim.size as usize);
                            return Ok(decode(&bytes, prim, order));
                        }
                        let child = usize::try_from(i).ok().and_then(|i| self.nodes[node].children.get(i).copied()).ok_or_else(|| problem(line, crate::i18n::tf!("{} has no entry {i}.", self.nodes[node].name)))?;
                        Ok(self.values[child].clone())
                    }
                    Value::Str(s) => usize::try_from(i).ok().and_then(|i| s.get(i)).map(|b| Value::Int(i128::from(*b))).ok_or_else(|| problem(line, crate::i18n::tf!("The text has no character {i}."))),
                    _ => Err(problem(line, crate::i18n::tr("Only a list can be indexed with [ ].").into())),
                }
            }
            Expr::Call(name, args) => self.call(name, args, line),
            Expr::Unary(op, inner) => match (*op, self.eval(inner, line)?) {
                ("-", Value::Int(n)) => Ok(Value::Int(-n)),
                ("-", Value::Float(f)) => Ok(Value::Float(-f)),
                ("~", Value::Int(n)) => Ok(Value::Int(!n)),
                ("!", value) => Ok(Value::Int(i128::from(!value.truthy()))),
                _ => Err(problem(line, crate::i18n::tf!("{op} works only on a number."))),
            },
            Expr::Binary(op, a, b) => {
                // && and || look at their right side only when they need to, so it can guard against reading too far.
                if *op == "&&" || *op == "||" {
                    let left = self.eval(a, line)?.truthy();
                    if (*op == "&&") != left {
                        return Ok(Value::Int(i128::from(left)));
                    }
                    return Ok(Value::Int(i128::from(self.eval(b, line)?.truthy())));
                }
                let (a, b) = (self.eval(a, line)?, self.eval(b, line)?);
                binary(op, a, b).map_err(|message| problem(line, message))
            }
            Expr::Cond(test, yes, no) => {
                if self.eval(test, line)?.truthy() {
                    self.eval(yes, line)
                } else {
                    self.eval(no, line)
                }
            }
        }
    }

    fn call(&mut self, name: &str, args: &[Expr], line: usize) -> Result<Value, Problem> {
        let wrong = |n: usize| problem(line, crate::i18n::tf!("{name}( ) takes {n}."));
        if let Some(prim) = prim_named(name) {
            let [at] = args else {
                return Err(wrong(1));
            };
            let at = self.int(at, line)?;
            let value = self.read_at(at, prim, line)?;
            if prim.num == Num::Char {
                return Ok(Value::Str(match value {
                    Value::Int(n) => vec![n as u8],
                    _ => Vec::new(),
                }));
            }
            return Ok(value);
        }
        match name {
            "start_of" | "end_of" | "size_of" | "count" => {
                let [field] = args else {
                    return Err(wrong(1));
                };
                let node = self.node_of(field, line)?;
                let n = &self.nodes[node];
                Ok(Value::Int(match name {
                    "start_of" => i128::from(n.start),
                    "end_of" => i128::from(n.end),
                    "size_of" => i128::from(n.end - n.start),
                    _ => match n.element {
                        Some((prim, ..)) => i128::from((n.end - n.start) / u64::from(prim.size)),
                        None => n.children.len() as i128,
                    },
                }))
            }
            "exists" => {
                let [field] = args else {
                    return Err(wrong(1));
                };
                Ok(Value::Int(i128::from(match field {
                    Expr::Name(name) => self.lookup(name, line).is_ok(),
                    other => self.node_of(other, line).is_ok(),
                })))
            }
            "str" => {
                let [at, count] = args else {
                    return Err(wrong(2));
                };
                let (at, count) = (self.int(at, line)?, self.int(count, line)?);
                Ok(Value::Str(self.bytes_at(at, count, line)?))
            }
            "cstr" => {
                let [at] = args else {
                    return Err(wrong(1));
                };
                let at = self.int(at, line)?;
                let mut bytes = self.bytes_at(at, 0, line)?;
                let start = at as u64;
                let chunk = self.src.read(start, 256);
                bytes.extend(chunk.iter().take_while(|b| **b != 0));
                Ok(Value::Str(bytes))
            }
            "xor" | "sum" | "crc32" => {
                let [at, count] = args else {
                    return Err(wrong(2));
                };
                let (at, count) = (self.int(at, line)?, self.int(count, line)?);
                if count > 64 * 1024 * 1024 {
                    return Err(problem(line, crate::i18n::tf!("{name}( ) works on at most 64 MB.")));
                }
                let bytes = self.bytes_at(at, count, line)?;
                Ok(Value::Int(match name {
                    "xor" => i128::from(bytes.iter().fold(0u8, |a, b| a ^ b)),
                    "sum" => bytes.iter().map(|b| i128::from(*b)).sum(),
                    _ => i128::from(crate::binary_tools::crc32(&bytes)),
                }))
            }
            "min" | "max" | "align" => {
                let [a, b] = args else {
                    return Err(wrong(2));
                };
                let (a, b) = (self.int(a, line)?, self.int(b, line)?);
                Ok(Value::Int(match name {
                    "min" => a.min(b),
                    "max" => a.max(b),
                    _ if b <= 0 => return Err(problem(line, crate::i18n::tr("align( ) needs a size above 0.").into())),
                    _ => (a + b - 1) / b * b,
                }))
            }
            "abs" => {
                let [a] = args else {
                    return Err(wrong(1));
                };
                Ok(Value::Int(self.int(a, line)?.abs()))
            }
            "len" => {
                let [a] = args else {
                    return Err(wrong(1));
                };
                match self.eval(a, line)? {
                    Value::Str(s) => Ok(Value::Int(s.len() as i128)),
                    _ => Err(problem(line, crate::i18n::tr("len( ) measures text.").into())),
                }
            }
            _ => {
                let Some((params, body, _)) = self.t.functions.get(name) else {
                    return Err(problem(line, crate::i18n::tf!("There's no function called {name}.")));
                };
                if params.len() != args.len() {
                    return Err(wrong(params.len()));
                }
                if self.calls >= MOST_DEPTH {
                    return Err(problem(line, crate::i18n::tf!("{name}( ) calls itself too deeply.")));
                }
                let mut vars = HashMap::new();
                for (param, arg) in params.iter().zip(args) {
                    vars.insert(param.clone(), self.eval(arg, line)?);
                }
                let order = self.order();
                self.scopes.push(Scope { vars, nodes: HashMap::new(), order });
                self.calls += 1;
                let result = self.eval(body, line);
                self.calls -= 1;
                self.scopes.pop();
                result
            }
        }
    }
}

fn leaf(name: &str, type_name: &str, start: u64, end: u64, shown: String, note: &Option<String>, kind: NodeKind) -> Node {
    Node { name: name.to_string(), type_name: type_name.to_string(), start, end, shown, note: note.clone().unwrap_or_default(), kind, children: Vec::new(), parent: None, depth: 0, colour: 0, element: None, line: 0 }
}

fn int_op(op: &str, a: i128, b: i128) -> Result<i128, String> {
    let overflow = || crate::i18n::tr("The number grew too large.").to_string();
    Ok(match op {
        "+" => a.checked_add(b).ok_or_else(overflow)?,
        "-" => a.checked_sub(b).ok_or_else(overflow)?,
        "*" => a.checked_mul(b).ok_or_else(overflow)?,
        "/" | "%" if b == 0 => return Err(crate::i18n::tr("That divides by nought.").into()),
        "/" => a / b,
        "%" => a.rem_euclid(b),
        "<<" => u32::try_from(b).ok().filter(|b| *b < 120).and_then(|b| a.checked_shl(b)).ok_or_else(overflow)?,
        ">>" => u32::try_from(b).ok().map_or(0, |b| if b >= 127 { 0 } else { a >> b }),
        "&" => a & b,
        "|" => a | b,
        "^" => a ^ b,
        "==" => i128::from(a == b),
        "!=" => i128::from(a != b),
        "<" => i128::from(a < b),
        "<=" => i128::from(a <= b),
        ">" => i128::from(a > b),
        ">=" => i128::from(a >= b),
        _ => return Err(crate::i18n::tf!("{op} isn't known.")),
    })
}

fn binary(op: &str, a: Value, b: Value) -> Result<Value, String> {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => int_op(op, a, b).map(Value::Int),
        (Value::Str(a), Value::Str(b)) => match op {
            "==" => Ok(Value::Int(i128::from(a == b))),
            "!=" => Ok(Value::Int(i128::from(a != b))),
            "+" => Ok(Value::Str([a, b].concat())),
            _ => Err(crate::i18n::tf!("{op} doesn't work on text.")),
        },
        (Value::Str(s), Value::Int(n)) | (Value::Int(n), Value::Str(s)) if (op == "==" || op == "!=") && s.len() == 1 => Ok(Value::Int(i128::from((i128::from(s[0]) == n) == (op == "==")))),
        (a @ (Value::Int(_) | Value::Float(_)), b @ (Value::Int(_) | Value::Float(_))) => {
            let float = |v: Value| match v {
                Value::Int(n) => n as f64,
                Value::Float(f) => f,
                _ => 0.0,
            };
            let (a, b) = (float(a), float(b));
            Ok(match op {
                "+" => Value::Float(a + b),
                "-" => Value::Float(a - b),
                "*" => Value::Float(a * b),
                "/" => Value::Float(a / b),
                "==" => Value::Int(i128::from(a == b)),
                "!=" => Value::Int(i128::from(a != b)),
                "<" => Value::Int(i128::from(a < b)),
                "<=" => Value::Int(i128::from(a <= b)),
                ">" => Value::Int(i128::from(a > b)),
                ">=" => Value::Int(i128::from(a >= b)),
                _ => return Err(crate::i18n::tf!("{op} doesn't work on numbers with fractions.")),
            })
        }
        _ => Err(crate::i18n::tr("A number and some text can't be put together like that.").into()),
    }
}

fn decode(bytes: &[u8], prim: Prim, order: Order) -> Value {
    let mut raw: u64 = 0;
    for i in 0..prim.size as usize {
        let byte = if order == Order::Big { bytes[i] } else { bytes[prim.size as usize - 1 - i] };
        raw = raw << 8 | u64::from(byte);
    }
    let bits = u32::from(prim.size) * 8;
    match prim.num {
        Num::Unsigned | Num::Char => Value::Int(i128::from(raw)),
        Num::Signed => {
            let shift = 64 - bits;
            Value::Int(i128::from(((raw << shift) as i64) >> shift))
        }
        Num::Float if prim.size == 4 => Value::Float(f64::from(f32::from_bits(raw as u32))),
        Num::Float => Value::Float(f64::from_bits(raw)),
    }
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|b| char::from(*b)).collect()
}

/// Text from a file, in quotes, with what can't be shown as a middle dot and long text cut short.
fn show_text(bytes: &[u8]) -> String {
    const MOST: usize = 80;
    let mut out = String::from("\"");
    // Names padded with spaces, or with 0xA0 as Commodore's are, are shown without the padding.
    let trimmed = bytes.iter().rposition(|b| *b != 0xA0 && *b != b' ').map_or(&bytes[..0], |end| &bytes[..=end]);
    for b in trimmed.iter().take(MOST) {
        out.push(if (0x20..0x7F).contains(b) || *b >= 0xA1 { char::from(*b) } else { '\u{B7}' });
    }
    out.push('"');
    if trimmed.len() > MOST {
        out.push('\u{2026}');
    }
    out
}

fn show_value(value: &Value, format: &Format, prim: Prim, enums: &HashMap<String, EnumDef>) -> String {
    let n = match value {
        Value::Int(n) => *n,
        Value::Float(f) => return f.to_string(),
        Value::Str(s) => return show_text(s),
        Value::Node(_) => return String::new(),
    };
    let digits = prim.size as usize * 2;
    let unsigned = if n < 0 { (n as u128 & ((1u128 << (prim.size as u32 * 8)) - 1)) as i128 } else { n };
    match format {
        Format::Plain if prim.num == Num::Signed || (0..10).contains(&n) => n.to_string(),
        Format::Plain => format!("{n} (0x{n:X})"),
        Format::Dec => n.to_string(),
        Format::Hex => format!("0x{unsigned:0digits$X}"),
        Format::Bin => format!("0b{unsigned:0width$b}", width = prim.size as usize * 8),
        Format::Oct => format!("0o{unsigned:o}"),
        Format::Char => match u8::try_from(n) {
            Ok(b) if (0x20..0x7F).contains(&b) => format!("'{}' ({n})", char::from(b)),
            _ => n.to_string(),
        },
        Format::DosDate => format!("{:04}-{:02}-{:02}", 1980 + (n >> 9), (n >> 5) & 15, n & 31),
        Format::DosTime => format!("{:02}:{:02}:{:02}", n >> 11, (n >> 5) & 63, (n & 31) * 2),
        Format::Unix => i64::try_from(n).ok().and_then(|s| chrono::DateTime::from_timestamp(s, 0)).map_or_else(|| n.to_string(), |t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
        Format::AmigaDays => chrono::NaiveDate::from_ymd_opt(1978, 1, 1).and_then(|start| start.checked_add_days(chrono::Days::new(u64::try_from(n).ok()?))).map_or_else(|| n.to_string(), |d| d.format("%Y-%m-%d").to_string()),
        Format::Named(name) => {
            let Some(def) = enums.get(name) else {
                return n.to_string();
            };
            if def.flags {
                let set: Vec<&str> = def.entries.iter().filter(|(_, bits, _)| *bits != 0 && n & bits == *bits).map(|(name, _, _)| name.as_str()).collect();
                let words = if set.is_empty() { crate::i18n::tr("none").to_string() } else { set.join(" | ") };
                format!("{words} (0x{unsigned:X})")
            } else {
                // What's said about the entry is shown when the pointer is on the field, keeping the value short.
                match def.entries.iter().find(|(_, v, _)| *v == n) {
                    Some((name, _, _)) if (0..10).contains(&n) => format!("{name} ({n})"),
                    Some((name, _, _)) => format!("{name} (0x{unsigned:X})"),
                    None => crate::i18n::tf!("{n} (0x{unsigned:X}), not one listed"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(template: &str, bytes: &[u8]) -> Tree {
        Template::parse(template).unwrap_or_else(|p| panic!("line {}: {}", p.line, p.message)).read(&Bytes(bytes))
    }

    fn shown(tree: &Tree, path: &str) -> String {
        let i = (1..tree.nodes.len()).find(|i| tree.path(*i) == path).unwrap_or_else(|| panic!("no {path} in {:?}", (1..tree.nodes.len()).map(|i| tree.path(i)).collect::<Vec<_>>()));
        tree.nodes[i].shown.clone()
    }

    fn node<'t>(tree: &'t Tree, path: &str) -> &'t Node {
        let i = (1..tree.nodes.len()).find(|i| tree.path(*i) == path).unwrap();
        &tree.nodes[i]
    }

    #[test]
    fn numbers_are_read_in_the_order_asked_for() {
        let t = "name \"t\"\norder little\nu16 a\nu16be b\nu32 c : hex\ns8 d\ns16 e\nu24 f\norder big\nu16 g\nf32 h";
        let bytes = [0x34, 0x12, 0x12, 0x34, 0x78, 0x56, 0x34, 0x12, 0xFF, 0xFE, 0xFF, 1, 2, 3, 0xAB, 0xCD, 0x3F, 0xC0, 0, 0];
        let tree = read(t, &bytes);
        assert_eq!(tree.error, None);
        assert_eq!(shown(&tree, "a"), "4660 (0x1234)");
        assert_eq!(shown(&tree, "b"), "4660 (0x1234)");
        assert_eq!(shown(&tree, "c"), "0x12345678");
        assert_eq!(shown(&tree, "d"), "-1");
        assert_eq!(shown(&tree, "e"), "-2");
        assert_eq!(shown(&tree, "f"), "197121 (0x30201)");
        assert_eq!(shown(&tree, "g"), "43981 (0xABCD)");
        assert_eq!(shown(&tree, "h"), "1.5");
        assert_eq!((node(&tree, "c").start, node(&tree, "c").end), (4, 8));
    }

    #[test]
    fn structs_arrays_and_counts_from_earlier_fields() {
        let t = r#"
name "list"
struct Item {
    u8 kind
    u8 length
    char text[length]
    label text
}
u8 count
Item items[count]
u8 rest[]
"#;
        let bytes = b"\x02\x01\x03abc\x07\x02hiXYZ";
        let tree = read(t, bytes);
        assert_eq!(tree.error, None);
        assert_eq!(shown(&tree, "items"), "2 entries");
        assert_eq!(shown(&tree, "items[0].text"), "\"abc\"");
        assert_eq!(shown(&tree, "items[1].text"), "\"hi\"");
        assert_eq!(shown(&tree, "items[1]"), "\"hi\"");
        assert_eq!(shown(&tree, "rest"), "58 59 5A");
        let item = node(&tree, "items[1]");
        assert_eq!((item.start, item.end), (6, 10));
        assert_eq!(tree.path(tree.field_at(8).unwrap()), "items[1].text");
    }

    #[test]
    fn conditions_enums_flags_and_while_lists() {
        let t = r#"
name "chunks"
order little
enum Kind : u8 {
    TEXT = 1
    NUMBER = 2 "A number"
}
flags Bits : u8 {
    READ = 1
    WRITE = 1 << 1
    RUN = 4
}
struct Chunk {
    Kind kind
    if kind == TEXT {
        cstring words
    } else if kind == NUMBER {
        u16 value
    }
    else {
        stop "An unknown chunk"
    }
}
Bits access
Chunk chunks[while u8(here) != 0]
u8 end
"#;
        let bytes = b"\x05\x01hi\x00\x02\x34\x12\x00";
        let tree = read(t, bytes);
        assert_eq!(tree.error, None);
        assert_eq!(shown(&tree, "access"), "READ | RUN (0x5)");
        assert_eq!(shown(&tree, "chunks[0].kind"), "TEXT (1)");
        assert_eq!(shown(&tree, "chunks[1].kind"), "NUMBER (2)");
        assert_eq!(node(&tree, "chunks[1].kind").note, "A number");
        assert_eq!(shown(&tree, "chunks[1].value"), "4660 (0x1234)");
        assert_eq!(shown(&tree, "chunks"), "2 entries");
        assert_eq!(node(&tree, "end").start, 8);
        // An unknown kind stops the reading where it is, with a note, and isn't a mistake in the template.
        let tree = read(t, b"\x00\x09");
        assert_eq!(tree.error, None);
        assert!(tree.nodes.iter().any(|n| n.kind == NodeKind::Warning && n.shown == "An unknown chunk"));
    }

    #[test]
    fn fields_at_offsets_follow_pointers_without_moving_on() {
        let t = r#"
name "pointers"
order big
u16 table
u16 entries[2] @ table
u8 after
calc second = entries[1] * 2
let base = 4
char tag[2] @ base + 2
check tag == "OK" "The tag is wrong"
check tag == "NO" "Not NO"
"#;
        let bytes = [0, 4, 9, 0, 0, 1, b'O', b'K'];
        let tree = read(t, &bytes);
        assert_eq!(tree.error, None);
        assert_eq!(shown(&tree, "entries"), "[1, 20299]");
        assert_eq!(node(&tree, "after").start, 2);
        assert_eq!(shown(&tree, "second"), "40598");
        assert_eq!(shown(&tree, "tag"), "\"OK\"");
        let warnings: Vec<&str> = tree.nodes.iter().filter(|n| n.kind == NodeKind::Warning).map(|n| n.shown.as_str()).collect();
        assert_eq!(warnings, ["Not NO"]);
        // The hover names one number in a list held as one field.
        assert_eq!(tree.describe(5, &Bytes(&bytes)).as_deref(), Some("entries[0]: 1"));
        // Where fields overlap, the one starting later is named.
        assert_eq!(tree.describe(6, &Bytes(&bytes)).as_deref(), Some("tag: \"OK\""));
    }

    #[test]
    fn expressions_have_the_usual_precedence() {
        let t = "name \"e\"\ndef double(x) = x * 2\ncalc a = 1 + 2 * 3\ncalc b = (1 + 2) * 3\ncalc c = 1 << 4 | 1\ncalc d = 7 > 3 && 2 > 1 ? 10 : 20\ncalc e = -7 % 3\ncalc f = $FF + 0b11 + 'A'\ncalc g = double(double(3))\ncalc h = min(4, 2) + max(1, 9) + align(5, 4)\ncalc i = \"ab\" + \"c\"\ncalc j = !0 + ~0\ncalc k = 6 & 2 != 0";
        let tree = read(t, &[]);
        assert_eq!(tree.error, None);
        let values: Vec<String> = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"].iter().map(|n| shown(&tree, n)).collect();
        assert_eq!(values, ["7", "9", "17", "10", "2", "323", "12", "19", "\"abc\"", "0", "1"]);
    }

    #[test]
    fn mistakes_in_a_template_are_found_with_their_line() {
        let cases = [
            ("u8 a", 1, "A template needs a name"),
            ("name \"x\"\nu8 a\nwidget b", 3, "There's no type called widget."),
            ("name \"x\"\nu8 a[3", 2, "']' was expected"),
            ("name \"x\"\n\nu16 a : sideways", 3, "sideways isn't a way of showing a value"),
            ("name \"x\"\nstruct A {\n  u8 a\n  u8 b c\n}", 4, "The end of the line was expected"),
            ("name \"x\"\nu8 a = \"unfinished", 2, "This text has no closing quote."),
            ("name \"x\"\nu8 a\nstruct A { }\nstruct A { }", 4, "There's already a type called A."),
        ];
        for (text, line, words) in cases {
            let p = Template::parse(text).expect_err(text);
            assert_eq!(p.line, line, "{text}: {}", p.message);
            assert!(p.message.contains(words), "{text}: {}", p.message);
        }
    }

    #[test]
    fn mistakes_found_while_reading_keep_what_was_read() {
        let t = "name \"x\"\nu8 count\nu8 data[count]\nu8 more\ncalc z = nothing + 1";
        let tree = read(t, &[2, 1, 2, 3]);
        assert_eq!(tree.error.as_ref().map(|p| p.line), Some(5));
        assert!(tree.error.as_ref().unwrap().message.contains("Nothing called nothing"));
        assert_eq!(shown(&tree, "more"), "3");
        let tree = read(t, &[9, 1, 2]);
        let error = tree.error.unwrap();
        assert_eq!(error.line, 3);
        assert!(error.message.contains("'data' would run past the end of the file"), "{}", error.message);
        // A list that never moves on is stopped rather than going round for ever.
        let tree = read("name \"x\"\nstruct E { let a = 1 }\nE e[]", &[1, 2]);
        assert!(tree.error.unwrap().message.contains("took up no bytes"));
        // A struct inside itself goes only so deep.
        let tree = read("name \"x\"\nstruct R { u8 a\n R r }\nR r", &[0; 100]);
        assert!(tree.error.unwrap().message.contains("more than 64 deep"));
    }

    #[test]
    fn magic_and_endings_choose_the_files() {
        let t = Template::parse("name \"x\"\nendings png \"8svx\"\nmagic str(1, 3) == \"PNG\"\nu8 a").unwrap();
        assert_eq!(t.endings, ["png", "8svx"]);
        assert!(t.fits(&Bytes(b"\x89PNG"), "png"));
        assert!(t.fits(&Bytes(b"\x89PNG"), "8SVX"));
        assert!(!t.fits(&Bytes(b"\x89PNG"), "gif"));
        assert!(!t.fits(&Bytes(b"\x89GIF"), "png"));
        // Too short a file isn't an error, only not the format.
        assert!(!t.fits(&Bytes(b"\x89"), "png"));
        let any = Template::parse("name \"y\"\nmagic u32be(0) == 0x3F3").unwrap();
        assert!(any.fits(&Bytes(&[0u8, 0, 3, 0xF3]), ""));
        assert!(!Template::parse("name \"z\"\nu8 a").unwrap().fits(&Bytes(&[0u8]), "bin"));
    }

    #[test]
    fn dates_times_and_members() {
        let t = "name \"d\"\norder little\nstruct T { u16 time : dostime\n u16 date : dosdate }\nT when\nu32 unix : unix\ncalc year = 1980 + (when.date >> 9)\ncalc length = size_of(when) + start_of(unix) + end_of(when.date)\n";
        let mut bytes = vec![0x62, 0x5A, 0x4B, 0x53];
        bytes.extend(1_000_000_000u32.to_le_bytes());
        let tree = read(t, &bytes);
        assert_eq!(tree.error, None);
        assert_eq!(shown(&tree, "when.time"), "11:19:04");
        assert_eq!(shown(&tree, "when.date"), "2021-10-11");
        assert_eq!(shown(&tree, "unix"), "2001-09-09 01:46:40 UTC");
        assert_eq!(shown(&tree, "year"), "2021");
        assert_eq!(shown(&tree, "length"), "12");
    }
}
