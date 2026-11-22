//! Queries over the rows of a delimited file, in the SQL-like style of Rainbow CSV's RBQL:
//!
//! ```text
//! SELECT a, b WHERE c > 10 ORDER BY d DESC LIMIT 100
//! SELECT region, COUNT(*), SUM(sales) GROUP BY region
//! ```
//!
//! Columns are named by their headings (`` `unit price` `` or `"unit price"` when they have spaces), or `a1`, `a2`
//! and so on, which work whether or not the file has a header row. Text that reads as a number is compared and
//! added as one. Everything here is written by hand, so that a mistake can say where in the query it is.

use crate::data_table::{Cell, ColumnKind, DataColumn, DataTable};
use crate::table;
use std::cmp::Ordering;
use std::collections::HashMap;

/// What went wrong, and where in the query (a character position).
#[derive(Clone, Debug, PartialEq)]
pub struct QueryError {
    pub message: String,
    pub at: usize,
}

impl QueryError {
    fn new(message: String, at: usize) -> Self {
        QueryError { message, at }
    }

    /// The message with where it is, for the person to read.
    pub fn describe(&self, query: &str) -> String {
        let before: String = query.chars().take(self.at).collect();
        let line = before.matches('\n').count() + 1;
        let column = before.chars().rev().take_while(|c| *c != '\n').count() + 1;
        if line > 1 { crate::i18n::tf!("{message} (line {line}, character {column})", message = self.message, line = line, column = column) } else { crate::i18n::tf!("{message} (character {column})", message = self.message, column = column) }
    }
}

type Done<T> = Result<T, QueryError>;

// ---------- Reading the query ----------

#[derive(Clone, Debug, PartialEq)]
enum Token {
    /// A bare word: a keyword, a function or a column.
    Word(String),
    /// A column's name in backticks or double quotes.
    Name(String),
    Text(String),
    Number(f64),
    /// One of the symbols: ( ) , * + - / % = == != <> < <= > >= ||
    Symbol(&'static str),
    End,
}

struct Piece {
    token: Token,
    at: usize,
    /// Where it ends, to take the text of an expression from the query.
    to: usize,
}

fn lex(query: &str) -> Done<Vec<Piece>> {
    let chars: Vec<char> = query.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if c.is_whitespace() || c == ';' && chars[i..].iter().all(|c| c.is_whitespace() || *c == ';') {
            i += 1;
            continue;
        }
        let token = if c.is_alphabetic() || c == '_' {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            Token::Word(chars[start..i].iter().collect())
        } else if c.is_ascii_digit() || c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit) {
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && matches!(chars[i], 'e' | 'E') {
                let mut j = i + 1;
                if j < chars.len() && matches!(chars[j], '+' | '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    while j < chars.len() && chars[j].is_ascii_digit() {
                        j += 1;
                    }
                    i = j;
                }
            }
            let text: String = chars[start..i].iter().collect();
            Token::Number(text.parse().map_err(|_| QueryError::new(crate::i18n::tf!("{text} isn't a number", text = text), start))?)
        } else if matches!(c, '\'' | '"' | '`') {
            let mut value = String::new();
            i += 1;
            loop {
                let Some(&d) = chars.get(i) else {
                    let what = if c == '\'' { crate::i18n::tr("The text starting here never ends: it needs a closing '") } else { crate::i18n::tr("The column name starting here never ends: it needs a closing quote") };
                    return Err(QueryError::new(what.to_string(), start));
                };
                i += 1;
                if d == c {
                    // Two quotes together are one quote in the text.
                    if chars.get(i) == Some(&c) {
                        value.push(c);
                        i += 1;
                        continue;
                    }
                    break;
                }
                value.push(d);
            }
            if c == '\'' { Token::Text(value) } else { Token::Name(value) }
        } else {
            let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
            let symbol = match two.as_str() {
                "==" => Some("=="),
                "!=" => Some("!="),
                "<>" => Some("<>"),
                "<=" => Some("<="),
                ">=" => Some(">="),
                "||" => Some("||"),
                _ => None,
            };
            if let Some(symbol) = symbol {
                i += 2;
                Token::Symbol(symbol)
            } else {
                i += 1;
                match c {
                    '(' => Token::Symbol("("),
                    ')' => Token::Symbol(")"),
                    ',' => Token::Symbol(","),
                    '*' => Token::Symbol("*"),
                    '+' => Token::Symbol("+"),
                    '-' => Token::Symbol("-"),
                    '/' => Token::Symbol("/"),
                    '%' => Token::Symbol("%"),
                    '=' => Token::Symbol("="),
                    '<' => Token::Symbol("<"),
                    '>' => Token::Symbol(">"),
                    other => return Err(QueryError::new(crate::i18n::tf!("I don't know what {other} means here", other = other), start)),
                }
            }
        };
        out.push(Piece { token, at: start, to: i });
    }
    out.push(Piece { token: Token::End, at: chars.len(), to: chars.len() });
    Ok(out)
}

// ---------- What a query is ----------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Aggregate {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Function {
    Upper,
    Lower,
    Length,
    Trim,
    Abs,
    Round,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Compare {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Clone, Debug)]
struct Node {
    at: usize,
    kind: Kind,
}

#[derive(Clone, Debug)]
enum Kind {
    Number(f64),
    Text(String),
    Column(usize),
    Negate(Box<Node>),
    Calc(char, Box<Node>, Box<Node>),
    Join(Box<Node>, Box<Node>),
    Compare(Compare, Box<Node>, Box<Node>),
    And(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
    Not(Box<Node>),
    IsNull(Box<Node>, bool),
    In(Box<Node>, Vec<Node>, bool),
    Like(Box<Node>, Box<Node>, bool),
    Between(Box<Node>, Box<Node>, Box<Node>, bool),
    Call(Function, Vec<Node>),
    /// An aggregate over the rows of a group; None inside is COUNT(*).
    Total(Aggregate, Option<Box<Node>>),
}

struct Item {
    node: Node,
    name: String,
}

struct Order {
    node: Node,
    descending: bool,
}

/// A query, read and checked against the columns.
pub struct Query {
    everything: bool,
    distinct: bool,
    items: Vec<Item>,
    filter: Option<Node>,
    groups: Vec<Node>,
    having: Option<Node>,
    order: Vec<Order>,
    limit: Option<usize>,
    skip: usize,
    /// Whether the rows are put together in groups (there's a GROUP BY or an aggregate).
    grouped: bool,
    columns: Vec<String>,
}

struct Parser<'a> {
    pieces: Vec<Piece>,
    at: usize,
    query: &'a str,
    names: &'a [String],
    /// Inside an aggregate's brackets, where another aggregate makes no sense.
    inside_total: bool,
    /// Where aggregates aren't allowed (WHERE and GROUP BY).
    no_totals: Option<&'static str>,
}

const KEYWORDS: [&str; 22] = ["select", "from", "where", "group", "by", "having", "order", "limit", "offset", "asc", "desc", "and", "or", "not", "in", "like", "between", "is", "null", "as", "distinct", "top"];

fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word.to_ascii_lowercase().as_str())
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &Token {
        &self.pieces[self.at].token
    }

    fn here(&self) -> usize {
        self.pieces[self.at].at
    }

    fn next(&mut self) -> &Piece {
        let piece = &self.pieces[self.at];
        if self.at + 1 < self.pieces.len() {
            self.at += 1;
        }
        piece
    }

    fn is_word(&self, word: &str) -> bool {
        matches!(self.peek(), Token::Word(w) if w.eq_ignore_ascii_case(word))
    }

    fn eat_word(&mut self, word: &str) -> bool {
        let found = self.is_word(word);
        if found {
            self.next();
        }
        found
    }

    fn is_symbol(&self, symbol: &str) -> bool {
        matches!(self.peek(), Token::Symbol(s) if *s == symbol)
    }

    fn eat_symbol(&mut self, symbol: &str) -> bool {
        let found = self.is_symbol(symbol);
        if found {
            self.next();
        }
        found
    }

    /// What's under the cursor, in words, for a message.
    fn found(&self) -> String {
        match self.peek() {
            Token::End => crate::i18n::tr("the end of the query").to_string(),
            Token::Word(w) => format!("\u{2018}{w}\u{2019}"),
            Token::Name(n) => format!("\u{2018}{n}\u{2019}"),
            Token::Text(t) => format!("'{t}'"),
            Token::Number(_) => self.query.chars().skip(self.pieces[self.at].at).take(self.pieces[self.at].to - self.pieces[self.at].at).collect(),
            Token::Symbol(s) => format!("\u{2018}{s}\u{2019}"),
        }
    }

    fn expected(&self, what: &str) -> QueryError {
        QueryError::new(crate::i18n::tf!("Expected {what}, but found {found}", what = what, found = self.found()), self.here())
    }

    fn expect_word(&mut self, word: &str) -> Done<()> {
        if self.eat_word(word) { Ok(()) } else { Err(self.expected(&word.to_ascii_uppercase())) }
    }

    fn expect_symbol(&mut self, symbol: &str) -> Done<()> {
        if self.eat_symbol(symbol) { Ok(()) } else { Err(self.expected(&format!("\u{2018}{symbol}\u{2019}"))) }
    }

    fn column(&self, name: &str, at: usize) -> Done<usize> {
        if let Some(i) = self.names.iter().position(|n| n.eq_ignore_ascii_case(name)) {
            return Ok(i);
        }
        // a1, a2 and so on always work.
        let lower = name.to_ascii_lowercase();
        if let Some(n) = lower.strip_prefix('a').and_then(|n| n.parse::<usize>().ok()).filter(|n| *n >= 1 && lower.len() > 1)
            && n <= self.names.len()
        {
            return Ok(n - 1);
        }
        let shown: Vec<String> = self.names.iter().take(8).map(|n| format!("\u{2018}{n}\u{2019}")).collect();
        let more = if self.names.len() > 8 { ", \u{2026}" } else { "" };
        Err(QueryError::new(crate::i18n::tf!("There's no column called {name}. The columns are {list}{more}", name = name, list = shown.join(", "), more = more), at))
    }

    fn expression(&mut self) -> Done<Node> {
        self.or()
    }

    fn or(&mut self) -> Done<Node> {
        let mut left = self.and()?;
        while self.is_word("or") {
            let at = self.next().at;
            let right = self.and()?;
            left = Node { at, kind: Kind::Or(Box::new(left), Box::new(right)) };
        }
        Ok(left)
    }

    fn and(&mut self) -> Done<Node> {
        let mut left = self.not()?;
        while self.is_word("and") {
            let at = self.next().at;
            let right = self.not()?;
            left = Node { at, kind: Kind::And(Box::new(left), Box::new(right)) };
        }
        Ok(left)
    }

    fn not(&mut self) -> Done<Node> {
        if self.is_word("not") {
            let at = self.next().at;
            let inner = self.not()?;
            return Ok(Node { at, kind: Kind::Not(Box::new(inner)) });
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Done<Node> {
        let left = self.sum()?;
        let at = self.here();
        let operator = match self.peek() {
            Token::Symbol("=" | "==") => Some(Compare::Equal),
            Token::Symbol("!=" | "<>") => Some(Compare::NotEqual),
            Token::Symbol("<") => Some(Compare::Less),
            Token::Symbol("<=") => Some(Compare::LessOrEqual),
            Token::Symbol(">") => Some(Compare::Greater),
            Token::Symbol(">=") => Some(Compare::GreaterOrEqual),
            _ => None,
        };
        if let Some(operator) = operator {
            self.next();
            let right = self.sum()?;
            return Ok(Node { at, kind: Kind::Compare(operator, Box::new(left), Box::new(right)) });
        }
        if self.eat_word("is") {
            let negated = self.eat_word("not");
            self.expect_word("null")?;
            return Ok(Node { at, kind: Kind::IsNull(Box::new(left), negated) });
        }
        let negated = self.is_word("not") && matches!(&self.pieces[(self.at + 1).min(self.pieces.len() - 1)].token, Token::Word(w) if ["in", "like", "between"].contains(&w.to_ascii_lowercase().as_str()));
        if negated {
            self.next();
        }
        if self.eat_word("in") {
            self.expect_symbol("(")?;
            let mut list = vec![self.sum()?];
            while self.eat_symbol(",") {
                list.push(self.sum()?);
            }
            self.expect_symbol(")")?;
            return Ok(Node { at, kind: Kind::In(Box::new(left), list, negated) });
        }
        if self.eat_word("like") {
            let pattern = self.sum()?;
            return Ok(Node { at, kind: Kind::Like(Box::new(left), Box::new(pattern), negated) });
        }
        if self.eat_word("between") {
            let low = self.sum()?;
            self.expect_word("and")?;
            let high = self.sum()?;
            return Ok(Node { at, kind: Kind::Between(Box::new(left), Box::new(low), Box::new(high), negated) });
        }
        Ok(left)
    }

    fn sum(&mut self) -> Done<Node> {
        let mut left = self.product()?;
        loop {
            let at = self.here();
            let kind = match self.peek() {
                Token::Symbol("+") => '+',
                Token::Symbol("-") => '-',
                Token::Symbol("||") => '|',
                _ => return Ok(left),
            };
            self.next();
            let right = self.product()?;
            left = Node { at, kind: if kind == '|' { Kind::Join(Box::new(left), Box::new(right)) } else { Kind::Calc(kind, Box::new(left), Box::new(right)) } };
        }
    }

    fn product(&mut self) -> Done<Node> {
        let mut left = self.unary()?;
        loop {
            let at = self.here();
            let kind = match self.peek() {
                Token::Symbol("*") => '*',
                Token::Symbol("/") => '/',
                Token::Symbol("%") => '%',
                _ => return Ok(left),
            };
            self.next();
            let right = self.unary()?;
            left = Node { at, kind: Kind::Calc(kind, Box::new(left), Box::new(right)) };
        }
    }

    fn unary(&mut self) -> Done<Node> {
        if self.is_symbol("-") {
            let at = self.next().at;
            let inner = self.unary()?;
            return Ok(Node { at, kind: Kind::Negate(Box::new(inner)) });
        }
        self.primary()
    }

    fn primary(&mut self) -> Done<Node> {
        let at = self.here();
        match self.peek().clone() {
            Token::Number(n) => {
                self.next();
                Ok(Node { at, kind: Kind::Number(n) })
            }
            Token::Text(t) => {
                self.next();
                Ok(Node { at, kind: Kind::Text(t) })
            }
            Token::Name(name) => {
                self.next();
                Ok(Node { at, kind: Kind::Column(self.column(&name, at)?) })
            }
            Token::Symbol("(") => {
                self.next();
                let inner = self.expression()?;
                self.expect_symbol(")")?;
                Ok(inner)
            }
            Token::Word(word) => {
                let lower = word.to_ascii_lowercase();
                let aggregate = match lower.as_str() {
                    "count" => Some(Aggregate::Count),
                    "sum" => Some(Aggregate::Sum),
                    "avg" => Some(Aggregate::Avg),
                    "min" => Some(Aggregate::Min),
                    "max" => Some(Aggregate::Max),
                    _ => None,
                };
                let function = match lower.as_str() {
                    "upper" => Some(Function::Upper),
                    "lower" => Some(Function::Lower),
                    "length" | "len" => Some(Function::Length),
                    "trim" => Some(Function::Trim),
                    "abs" => Some(Function::Abs),
                    "round" => Some(Function::Round),
                    _ => None,
                };
                let called = matches!(&self.pieces[(self.at + 1).min(self.pieces.len() - 1)].token, Token::Symbol("("));
                if let (Some(aggregate), true) = (aggregate, called) {
                    self.next();
                    self.next();
                    if let Some(place) = self.no_totals {
                        return Err(QueryError::new(crate::i18n::tf!("{word} can't be used in {place}: it needs the rows grouped first", word = word.to_ascii_uppercase(), place = place), at));
                    }
                    if self.inside_total {
                        return Err(QueryError::new(crate::i18n::tr("An aggregate can't be inside another one").to_string(), at));
                    }
                    let inner = if self.eat_symbol("*") {
                        if aggregate != Aggregate::Count {
                            return Err(QueryError::new(crate::i18n::tf!("{word}(*) isn't allowed: only COUNT can count whole rows", word = word.to_ascii_uppercase()), at));
                        }
                        None
                    } else {
                        self.inside_total = true;
                        let inner = self.expression();
                        self.inside_total = false;
                        Some(Box::new(inner?))
                    };
                    self.expect_symbol(")")?;
                    return Ok(Node { at, kind: Kind::Total(aggregate, inner) });
                }
                if let (Some(function), true) = (function, called) {
                    self.next();
                    self.next();
                    let mut arguments = vec![self.expression()?];
                    while self.eat_symbol(",") {
                        arguments.push(self.expression()?);
                    }
                    self.expect_symbol(")")?;
                    let allowed = if function == Function::Round { 1..=2 } else { 1..=1 };
                    if !allowed.contains(&arguments.len()) {
                        return Err(QueryError::new(crate::i18n::tf!("{word} takes {count} here", word = word.to_ascii_uppercase(), count = if function == Function::Round { crate::i18n::tr("one or two values") } else { crate::i18n::tr("one value") }), at));
                    }
                    return Ok(Node { at, kind: Kind::Call(function, arguments) });
                }
                if is_keyword(&word) {
                    return Err(self.expected(crate::i18n::tr("a column, a number or some text")));
                }
                self.next();
                Ok(Node { at, kind: Kind::Column(self.column(&word, at)?) })
            }
            _ => Err(self.expected(crate::i18n::tr("a column, a number or some text"))),
        }
    }

    fn whole_number(&mut self, what: &str) -> Done<usize> {
        let at = self.here();
        match self.peek().clone() {
            Token::Number(n) if n >= 0.0 && n.fract() == 0.0 => {
                self.next();
                Ok(n as usize)
            }
            _ => Err(QueryError::new(crate::i18n::tf!("{what} needs a whole number, but found {found}", what = what, found = self.found()), at)),
        }
    }

    fn text_between(&self, from: usize, to: usize) -> String {
        self.query.chars().skip(from).take(to - from).collect::<String>().trim().to_string()
    }
}

fn has_total(node: &Node) -> bool {
    match &node.kind {
        Kind::Total(..) => true,
        Kind::Number(_) | Kind::Text(_) | Kind::Column(_) => false,
        Kind::Negate(a) | Kind::Not(a) | Kind::IsNull(a, _) => has_total(a),
        Kind::Calc(_, a, b) | Kind::Join(a, b) | Kind::Compare(_, a, b) | Kind::And(a, b) | Kind::Or(a, b) | Kind::Like(a, b, _) => has_total(a) || has_total(b),
        Kind::In(a, list, _) => has_total(a) || list.iter().any(has_total),
        Kind::Between(a, b, c, _) => has_total(a) || has_total(b) || has_total(c),
        Kind::Call(_, arguments) => arguments.iter().any(has_total),
    }
}

/// The columns used outside any aggregate, with where they're written.
fn loose_columns(node: &Node, out: &mut Vec<(usize, usize)>) {
    match &node.kind {
        Kind::Total(..) | Kind::Number(_) | Kind::Text(_) => {}
        Kind::Column(c) => out.push((*c, node.at)),
        Kind::Negate(a) | Kind::Not(a) | Kind::IsNull(a, _) => loose_columns(a, out),
        Kind::Calc(_, a, b) | Kind::Join(a, b) | Kind::Compare(_, a, b) | Kind::And(a, b) | Kind::Or(a, b) | Kind::Like(a, b, _) => {
            loose_columns(a, out);
            loose_columns(b, out);
        }
        Kind::In(a, list, _) => {
            loose_columns(a, out);
            list.iter().for_each(|n| loose_columns(n, out));
        }
        Kind::Between(a, b, c, _) => {
            loose_columns(a, out);
            loose_columns(b, out);
            loose_columns(c, out);
        }
        Kind::Call(_, arguments) => arguments.iter().for_each(|n| loose_columns(n, out)),
    }
}

/// How many columns the data has and what each is called: its headings, or a1, a2… for those without.
pub fn column_names(headings: Option<&[String]>, width: usize) -> Vec<String> {
    (0..width.max(headings.map_or(0, <[String]>::len))).map(|i| headings.and_then(|h| h.get(i)).filter(|h| !h.trim().is_empty()).map_or_else(|| format!("a{}", i + 1), |h| h.trim().to_string())).collect()
}

/// Reads and checks a query against the columns' names.
pub fn parse(query: &str, names: &[String]) -> Done<Query> {
    let mut p = Parser { pieces: lex(query)?, at: 0, query, names, inside_total: false, no_totals: None };
    p.expect_word("select")?;
    let distinct = p.eat_word("distinct");
    let mut everything = false;
    let mut items = Vec::new();
    loop {
        if p.is_symbol("*") {
            p.next();
            everything = true;
        } else {
            let from = p.here();
            let node = p.expression()?;
            let to = p.pieces[p.at.saturating_sub(1)].to;
            let name = if p.eat_word("as") {
                match p.peek().clone() {
                    Token::Word(n) | Token::Name(n) | Token::Text(n) => {
                        p.next();
                        n
                    }
                    _ => return Err(p.expected(crate::i18n::tr("a name for the column"))),
                }
            } else if let Kind::Column(c) = node.kind {
                names[c].clone()
            } else {
                p.text_between(from, to)
            };
            items.push(Item { node, name });
        }
        if !p.eat_symbol(",") {
            break;
        }
    }
    // The file is the one table; naming it is allowed, as it is in Rainbow CSV's own queries.
    if p.eat_word("from") {
        match p.peek() {
            Token::Word(w) if !is_keyword(w) => {
                p.next();
            }
            Token::Name(_) => {
                p.next();
            }
            _ => return Err(p.expected(crate::i18n::tr("a name after FROM"))),
        }
    }
    let mut filter = None;
    let mut groups = Vec::new();
    let mut having = None;
    let mut order = Vec::new();
    let mut limit = None;
    let mut skip = 0;
    if p.eat_word("where") {
        p.no_totals = Some("WHERE");
        filter = Some(p.expression()?);
        p.no_totals = None;
    }
    if p.eat_word("group") {
        p.expect_word("by")?;
        p.no_totals = Some("GROUP BY");
        loop {
            groups.push(p.expression()?);
            if !p.eat_symbol(",") {
                break;
            }
        }
        p.no_totals = None;
    }
    if p.eat_word("having") {
        having = Some(p.expression()?);
    }
    if p.eat_word("order") {
        p.expect_word("by")?;
        loop {
            let at = p.here();
            // A whole number picks a column of the result; a name of one picks that column too.
            let node = match p.peek().clone() {
                Token::Number(n) if n >= 1.0 && n.fract() == 0.0 => {
                    p.next();
                    let item = items.get(n as usize - 1).ok_or_else(|| QueryError::new(crate::i18n::tf!("The result has no column number {n}", n = n as usize), at))?;
                    item.node.clone()
                }
                Token::Word(w) | Token::Name(w) if items.iter().any(|i| i.name.eq_ignore_ascii_case(&w)) && !p.names.iter().any(|n| n.eq_ignore_ascii_case(&w)) => {
                    p.next();
                    items.iter().find(|i| i.name.eq_ignore_ascii_case(&w)).map(|i| i.node.clone()).unwrap_or(Node { at, kind: Kind::Number(0.0) })
                }
                _ => p.expression()?,
            };
            let descending = if p.eat_word("desc") {
                true
            } else {
                p.eat_word("asc");
                false
            };
            order.push(Order { node, descending });
            if !p.eat_symbol(",") {
                break;
            }
        }
    }
    if p.eat_word("limit") {
        limit = Some(p.whole_number("LIMIT")?);
        if p.eat_word("offset") {
            skip = p.whole_number("OFFSET")?;
        }
    }
    if !matches!(p.peek(), Token::End) {
        return Err(p.expected(crate::i18n::tr("WHERE, GROUP BY, ORDER BY, LIMIT or the end of the query")));
    }
    if items.is_empty() && !everything {
        return Err(QueryError::new(crate::i18n::tr("Choose some columns after SELECT, or * for all of them").to_string(), 0));
    }
    if everything && items.iter().any(|i| has_total(&i.node)) {
        return Err(QueryError::new(crate::i18n::tr("* can't be mixed with aggregates: name the columns").to_string(), 0));
    }
    let grouped = !groups.is_empty() || items.iter().any(|i| has_total(&i.node)) || having.is_some();
    if grouped {
        if everything {
            return Err(QueryError::new(crate::i18n::tr("* can't be used when grouping: name the columns").to_string(), 0));
        }
        // Each column outside an aggregate must be one the rows are grouped by.
        let keys: Vec<usize> = groups.iter().filter_map(|g| if let Kind::Column(c) = g.kind { Some(c) } else { None }).collect();
        let mut loose = Vec::new();
        items.iter().for_each(|i| loose_columns(&i.node, &mut loose));
        if let Some(h) = &having {
            loose_columns(h, &mut loose);
        }
        for (column, at) in loose {
            if !keys.contains(&column) {
                return Err(QueryError::new(crate::i18n::tf!("{name} isn't in GROUP BY, so it needs an aggregate such as COUNT or MAX around it", name = format!("\u{2018}{}\u{2019}", names[column])), at));
            }
        }
    }
    Ok(Query { everything, distinct, items, filter, groups, having, order, limit, skip, grouped, columns: names.to_vec() })
}

// ---------- Working it out ----------

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Null,
    Number(f64),
    Text(String),
    Bool(bool),
}

impl Value {
    fn text(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Number(n) => show_number(*n),
            Value::Text(t) => t.clone(),
            Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        }
    }

    /// As a number, if it is one or reads as one.
    fn number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            Value::Text(t) => table::number(t),
            Value::Bool(b) => Some(f64::from(u8::from(*b))),
            Value::Null => None,
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Value::Null => true,
            Value::Text(t) => t.trim().is_empty(),
            _ => false,
        }
    }
}

fn show_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        let text = format!("{n:.10}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Numbers are compared as numbers when both sides are; otherwise as text.
fn compare(a: &Value, b: &Value) -> Ordering {
    match (a.number(), b.number()) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => a.text().cmp(&b.text()),
    }
}

struct Run<'a> {
    rows: &'a [Vec<String>],
    names: &'a [String],
}

impl Run<'_> {
    fn cell(&self, row: usize, column: usize) -> Value {
        Value::Text(self.rows[row].get(column).cloned().unwrap_or_default())
    }

    fn number_of(&self, node: &Node, value: &Value, what: &str) -> Done<Option<f64>> {
        if value.is_empty() {
            return Ok(None);
        }
        value.number().map(Some).ok_or_else(|| QueryError::new(crate::i18n::tf!("{what} needs a number, but this is \u{201C}{text}\u{201D}", what = what, text = value.text()), node.at))
    }

    /// Works a node out for a set of rows: one row, or all the rows of a group.
    fn eval(&self, node: &Node, rows: &[usize]) -> Done<Value> {
        Ok(match &node.kind {
            Kind::Number(n) => Value::Number(*n),
            Kind::Text(t) => Value::Text(t.clone()),
            Kind::Column(c) => rows.first().map_or(Value::Null, |r| self.cell(*r, *c)),
            Kind::Negate(a) => {
                let v = self.eval(a, rows)?;
                match self.number_of(a, &v, crate::i18n::tr("Negating"))? {
                    Some(n) => Value::Number(-n),
                    None => Value::Null,
                }
            }
            Kind::Calc(op, a, b) => {
                let (x, y) = (self.eval(a, rows)?, self.eval(b, rows)?);
                let what = crate::i18n::tf!("The {op} sum", op = op);
                let (Some(x), Some(y)) = (self.number_of(a, &x, &what)?, self.number_of(b, &y, &what)?) else {
                    return Ok(Value::Null);
                };
                match op {
                    '+' => Value::Number(x + y),
                    '-' => Value::Number(x - y),
                    '*' => Value::Number(x * y),
                    '/' | '%' if y == 0.0 => return Err(QueryError::new(crate::i18n::tr("This divides by zero").to_string(), node.at)),
                    '/' => Value::Number(x / y),
                    _ => Value::Number(x % y),
                }
            }
            Kind::Join(a, b) => Value::Text(self.eval(a, rows)?.text() + self.eval(b, rows)?.text().as_str()),
            Kind::Compare(op, a, b) => {
                let (x, y) = (self.eval(a, rows)?, self.eval(b, rows)?);
                if x == Value::Null || y == Value::Null {
                    return Ok(Value::Bool(false));
                }
                let order = compare(&x, &y);
                Value::Bool(match op {
                    Compare::Equal => order == Ordering::Equal,
                    Compare::NotEqual => order != Ordering::Equal,
                    Compare::Less => order == Ordering::Less,
                    Compare::LessOrEqual => order != Ordering::Greater,
                    Compare::Greater => order == Ordering::Greater,
                    Compare::GreaterOrEqual => order != Ordering::Less,
                })
            }
            Kind::And(a, b) => Value::Bool(self.truth(a, rows)? && self.truth(b, rows)?),
            Kind::Or(a, b) => Value::Bool(self.truth(a, rows)? || self.truth(b, rows)?),
            Kind::Not(a) => Value::Bool(!self.truth(a, rows)?),
            Kind::IsNull(a, negated) => Value::Bool(self.eval(a, rows)?.is_empty() != *negated),
            Kind::In(a, list, negated) => {
                let x = self.eval(a, rows)?;
                let mut found = false;
                for item in list {
                    if compare(&x, &self.eval(item, rows)?) == Ordering::Equal {
                        found = true;
                        break;
                    }
                }
                Value::Bool(found != *negated)
            }
            Kind::Like(a, pattern, negated) => {
                let (x, p) = (self.eval(a, rows)?.text(), self.eval(pattern, rows)?.text());
                Value::Bool(like(&x.to_lowercase().chars().collect::<Vec<_>>(), &p.to_lowercase().chars().collect::<Vec<_>>()) != *negated)
            }
            Kind::Between(a, low, high, negated) => {
                let x = self.eval(a, rows)?;
                let inside = compare(&x, &self.eval(low, rows)?) != Ordering::Less && compare(&x, &self.eval(high, rows)?) != Ordering::Greater;
                Value::Bool(inside != *negated)
            }
            Kind::Call(function, arguments) => {
                let v = self.eval(&arguments[0], rows)?;
                match function {
                    Function::Upper => Value::Text(v.text().to_uppercase()),
                    Function::Lower => Value::Text(v.text().to_lowercase()),
                    Function::Trim => Value::Text(v.text().trim().to_string()),
                    Function::Length => Value::Number(v.text().chars().count() as f64),
                    Function::Abs => match self.number_of(&arguments[0], &v, "ABS")? {
                        Some(n) => Value::Number(n.abs()),
                        None => Value::Null,
                    },
                    Function::Round => {
                        let places = match arguments.get(1) {
                            Some(p) => self.eval(p, rows)?.number().unwrap_or(0.0).clamp(0.0, 12.0),
                            None => 0.0,
                        };
                        match self.number_of(&arguments[0], &v, "ROUND")? {
                            Some(n) => {
                                let scale = 10f64.powi(places as i32);
                                Value::Number((n * scale).round() / scale)
                            }
                            None => Value::Null,
                        }
                    }
                }
            }
            Kind::Total(aggregate, inner) => self.total(node, *aggregate, inner.as_deref(), rows)?,
        })
    }

    fn truth(&self, node: &Node, rows: &[usize]) -> Done<bool> {
        match self.eval(node, rows)? {
            Value::Bool(b) => Ok(b),
            Value::Null => Ok(false),
            Value::Number(n) => Ok(n != 0.0),
            Value::Text(_) => Err(QueryError::new(crate::i18n::tr("This should be a condition such as a > 3, not text").to_string(), node.at)),
        }
    }

    fn total(&self, node: &Node, aggregate: Aggregate, inner: Option<&Node>, rows: &[usize]) -> Done<Value> {
        let Some(inner) = inner else {
            return Ok(Value::Number(rows.len() as f64));
        };
        let mut values = Vec::with_capacity(rows.len());
        for &r in rows {
            let v = self.eval(inner, &[r])?;
            if !v.is_empty() {
                values.push(v);
            }
        }
        Ok(match aggregate {
            Aggregate::Count => Value::Number(values.len() as f64),
            Aggregate::Min | Aggregate::Max => {
                let best = values.into_iter().reduce(|a, b| {
                    let order = if all_numbers(&a, &b) { compare(&a, &b) } else { table::compare_values(&a.text(), &b.text()) };
                    if (aggregate == Aggregate::Min) == (order != Ordering::Greater) { a } else { b }
                });
                best.map_or(Value::Null, |v| match v.number() {
                    Some(n) if matches!(v, Value::Number(_)) => Value::Number(n),
                    _ => Value::Text(v.text()),
                })
            }
            Aggregate::Sum | Aggregate::Avg => {
                let mut sum = 0.0;
                for v in &values {
                    sum += self.number_of(node, v, if aggregate == Aggregate::Sum { "SUM" } else { "AVG" })?.unwrap_or(0.0);
                }
                if values.is_empty() {
                    Value::Null
                } else if aggregate == Aggregate::Sum {
                    Value::Number(sum)
                } else {
                    Value::Number(sum / values.len() as f64)
                }
            }
        })
    }
}

/// Text that is exactly how a whole number is written, so that making it one loses nothing.
fn whole_number(text: &str) -> Option<i64> {
    text.parse::<i64>().ok().filter(|n| n.to_string() == text)
}

fn all_numbers(a: &Value, b: &Value) -> bool {
    a.number().is_some() && b.number().is_some()
}

/// SQL's LIKE: `%` is any run of characters and `_` any one.
fn like(text: &[char], pattern: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('%', rest)) => (0..=text.len()).any(|i| like(&text[i..], rest)),
        Some(('_', rest)) => !text.is_empty() && like(&text[1..], rest),
        Some((c, rest)) => text.first() == Some(c) && like(&text[1..], rest),
    }
}

/// Runs a query over rows of text, giving a table. `headings` are the column names if the file has a header row.
#[cfg(test)]
pub fn run(query: &str, headings: Option<&[String]>, rows: &[Vec<String>]) -> Done<DataTable> {
    run_watched(query, headings, rows, &mut |_| true)
}

/// The same, telling how many rows it's got through every so often and stopping, with a mistake saying so, if told to.
pub fn run_watched(query: &str, headings: Option<&[String]>, rows: &[Vec<String>], going: &mut dyn FnMut(usize) -> bool) -> Done<DataTable> {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let names = column_names(headings, width);
    let parsed = parse(query, &names)?;
    parsed.run(rows, going)
}

impl Query {
    fn run(&self, rows: &[Vec<String>], going: &mut dyn FnMut(usize) -> bool) -> Done<DataTable> {
        let stopped = || QueryError::new(crate::i18n::tr("The query was stopped").to_string(), 0);
        let run = Run { rows, names: &self.columns };
        let _ = run.names;
        let mut kept: Vec<usize> = Vec::new();
        for r in 0..rows.len() {
            if r % 4096 == 0 && !going(r) {
                return Err(stopped());
            }
            if let Some(filter) = &self.filter
                && !run.truth(filter, &[r])?
            {
                continue;
            }
            kept.push(r);
        }
        // The sets of rows each result row is made from: every row alone, or the groups.
        let sets: Vec<Vec<usize>> = if !self.grouped {
            kept.iter().map(|r| vec![*r]).collect()
        } else if self.groups.is_empty() {
            vec![kept]
        } else {
            let mut index: HashMap<String, usize> = HashMap::new();
            let mut sets: Vec<Vec<usize>> = Vec::new();
            for (n, r) in kept.into_iter().enumerate() {
                if n % 4096 == 0 && !going(n) {
                    return Err(stopped());
                }
                let mut key = String::new();
                for g in &self.groups {
                    key.push_str(&run.eval(g, &[r])?.text());
                    key.push('\u{1f}');
                }
                let at = *index.entry(key).or_insert_with(|| {
                    sets.push(Vec::new());
                    sets.len() - 1
                });
                sets[at].push(r);
            }
            sets
        };
        let sets: Vec<Vec<usize>> = match &self.having {
            Some(having) => {
                let mut out = Vec::new();
                for set in sets {
                    if run.truth(having, &set)? {
                        out.push(set);
                    }
                }
                out
            }
            None => sets,
        };
        // Sorting is done on the sets, by keys worked out once for each.
        let mut sets = sets;
        if !self.order.is_empty() {
            let mut keyed: Vec<(Vec<String>, Vec<usize>)> = Vec::with_capacity(sets.len());
            for set in sets {
                let mut keys = Vec::with_capacity(self.order.len());
                for o in &self.order {
                    keys.push(run.eval(&o.node, &set)?.text());
                }
                keyed.push((keys, set));
            }
            keyed.sort_by(|(a, _), (b, _)| {
                for (i, o) in self.order.iter().enumerate() {
                    let order = table::compare_values(&a[i], &b[i]);
                    let order = if o.descending { order.reverse() } else { order };
                    if order != Ordering::Equal {
                        return order;
                    }
                }
                Ordering::Equal
            });
            sets = keyed.into_iter().map(|(_, set)| set).collect();
        }
        let width = self.columns.len();
        let mut headings: Vec<String> = Vec::new();
        if self.everything {
            headings.extend(self.columns.iter().cloned());
        }
        headings.extend(self.items.iter().map(|i| i.name.clone()));
        let mut out: Vec<Vec<Value>> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut skipped = 0;
        for set in sets.iter().take(if self.limit == Some(0) { 0 } else { usize::MAX }) {
            let mut row = Vec::with_capacity(headings.len());
            if self.everything {
                for c in 0..width {
                    row.push(run.cell(set[0], c));
                }
            }
            for item in &self.items {
                row.push(run.eval(&item.node, set)?);
            }
            if self.distinct && !seen.insert(row.iter().map(Value::text).collect::<Vec<_>>().join("\u{1f}")) {
                continue;
            }
            if skipped < self.skip {
                skipped += 1;
                continue;
            }
            out.push(row);
            if self.limit.is_some_and(|limit| out.len() >= limit) {
                break;
            }
        }
        let columns = (0..headings.len())
            .map(|c| {
                let values = out.iter().map(|row| &row[c]).filter(|v| **v != Value::Null && !v.is_empty());
                let mut kind = None;
                for v in values {
                    let this = match v {
                        Value::Number(n) if n.fract() == 0.0 => ColumnKind::Integer,
                        Value::Number(_) => ColumnKind::Real,
                        Value::Bool(_) => ColumnKind::Bool,
                        Value::Text(t) if whole_number(t).is_some() => ColumnKind::Integer,
                        _ => ColumnKind::Text,
                    };
                    kind = Some(match (kind, this) {
                        (None, k) => k,
                        (Some(a), b) if a == b => a,
                        (Some(ColumnKind::Integer | ColumnKind::Real), ColumnKind::Integer | ColumnKind::Real) => ColumnKind::Real,
                        _ => ColumnKind::Text,
                    });
                }
                DataColumn { name: headings[c].clone(), kind: kind.unwrap_or(ColumnKind::Text) }
            })
            .collect::<Vec<_>>();
        let rows = out
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .enumerate()
                    .map(|(c, v)| match (v, columns[c].kind) {
                        (Value::Null, _) => Cell::Null,
                        (Value::Number(n), ColumnKind::Integer) => Cell::Int(n as i64),
                        (Value::Text(t), ColumnKind::Integer) => Cell::Int(whole_number(&t).unwrap_or(0)),
                        (Value::Number(n), ColumnKind::Real) => Cell::Real(n),
                        (Value::Bool(b), ColumnKind::Bool) => Cell::Bool(b),
                        (v, _) => Cell::Text(v.text()),
                    })
                    .collect()
            })
            .collect();
        Ok(DataTable { columns, rows, note: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> (Vec<String>, Vec<Vec<String>>) {
        let head = ["name", "region", "sales", "unit price"].map(String::from).to_vec();
        let rows = [["Ann", "north", "10", "2.5"], ["Bob", "south", "25", "1.5"], ["Cy", "north", "7", "4"], ["Di", "south", "100", "1"], ["Ed", "east", "9", ""]].iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect();
        (head, rows)
    }

    fn go(query: &str) -> Result<DataTable, QueryError> {
        let (head, rows) = data();
        run(query, Some(&head), &rows)
    }

    fn text(table: &DataTable) -> Vec<Vec<String>> {
        table.rows.iter().map(|r| r.iter().map(Cell::text).collect()).collect()
    }

    fn column(table: &DataTable, c: usize) -> Vec<String> {
        table.rows.iter().map(|r| r[c].text()).collect()
    }

    #[test]
    fn select_picks_columns_in_the_order_asked() {
        let t = go("SELECT sales, name").unwrap();
        assert_eq!(t.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["sales", "name"]);
        assert_eq!(text(&t)[0], ["10", "Ann"]);
        assert_eq!(t.rows.len(), 5);
        assert_eq!(t.columns[0].kind, ColumnKind::Integer);
    }

    #[test]
    fn star_gives_every_column_and_keywords_ignore_case() {
        let t = go("select * where name = 'Cy'").unwrap();
        assert_eq!(text(&t), [["Cy", "north", "7", "4"]]);
        assert_eq!(t.columns.len(), 4);
    }

    #[test]
    fn where_compares_numbers_as_numbers() {
        // As text "100" would come before "9", and "25" before "7".
        let t = go("SELECT name WHERE sales > 9").unwrap();
        assert_eq!(column(&t, 0), ["Ann", "Bob", "Di"]);
        let t = go("SELECT name WHERE sales >= 9 AND sales <= 25").unwrap();
        assert_eq!(column(&t, 0), ["Ann", "Bob", "Ed"]);
        let t = go("SELECT name WHERE sales = 10.0").unwrap();
        assert_eq!(column(&t, 0), ["Ann"]);
    }

    #[test]
    fn where_has_and_or_not_and_brackets() {
        let t = go("SELECT name WHERE region = 'north' OR region = 'east' AND sales > 100").unwrap();
        assert_eq!(column(&t, 0), ["Ann", "Cy"]);
        let t = go("SELECT name WHERE (region = 'north' OR region = 'east') AND NOT sales < 8").unwrap();
        assert_eq!(column(&t, 0), ["Ann", "Ed"]);
    }

    #[test]
    fn where_has_in_like_between_and_is_null() {
        assert_eq!(column(&go("SELECT name WHERE region IN ('east', 'south')").unwrap(), 0), ["Bob", "Di", "Ed"]);
        assert_eq!(column(&go("SELECT name WHERE region NOT IN ('east', 'south')").unwrap(), 0), ["Ann", "Cy"]);
        assert_eq!(column(&go("SELECT name WHERE name LIKE '_o%'").unwrap(), 0), ["Bob"]);
        assert_eq!(column(&go("SELECT name WHERE region LIKE 'NO%'").unwrap(), 0), ["Ann", "Cy"]);
        assert_eq!(column(&go("SELECT name WHERE sales BETWEEN 9 AND 25").unwrap(), 0), ["Ann", "Bob", "Ed"]);
        assert_eq!(column(&go("SELECT name WHERE `unit price` IS NULL").unwrap(), 0), ["Ed"]);
        assert_eq!(column(&go("SELECT name WHERE \"unit price\" IS NOT NULL").unwrap(), 0), ["Ann", "Bob", "Cy", "Di"]);
    }

    #[test]
    fn order_by_sorts_numbers_as_numbers_and_desc_reverses() {
        assert_eq!(column(&go("SELECT sales ORDER BY sales").unwrap(), 0), ["7", "9", "10", "25", "100"]);
        assert_eq!(column(&go("SELECT sales ORDER BY sales DESC").unwrap(), 0), ["100", "25", "10", "9", "7"]);
        assert_eq!(column(&go("SELECT name ORDER BY region, sales DESC").unwrap(), 0), ["Ed", "Ann", "Cy", "Di", "Bob"]);
    }

    #[test]
    fn order_by_can_use_a_result_column_or_its_number() {
        assert_eq!(column(&go("SELECT name, sales * 2 AS double ORDER BY double DESC LIMIT 2").unwrap(), 0), ["Di", "Bob"]);
        assert_eq!(column(&go("SELECT name, sales ORDER BY 2 LIMIT 2").unwrap(), 0), ["Cy", "Ed"]);
    }

    #[test]
    fn limit_and_offset_trim_the_result() {
        assert_eq!(column(&go("SELECT name LIMIT 2").unwrap(), 0), ["Ann", "Bob"]);
        assert_eq!(column(&go("SELECT name LIMIT 2 OFFSET 3").unwrap(), 0), ["Di", "Ed"]);
        assert_eq!(go("SELECT name LIMIT 0").unwrap().rows.len(), 0);
    }

    #[test]
    fn the_clauses_work_together() {
        let t = go("SELECT a1, a3 WHERE sales > 8 ORDER BY sales DESC LIMIT 2").unwrap();
        assert_eq!(text(&t), [["Di", "100"], ["Bob", "25"]]);
    }

    #[test]
    fn aggregates_over_the_whole_file() {
        let t = go("SELECT COUNT(*), SUM(sales), AVG(sales), MIN(sales), MAX(sales)").unwrap();
        assert_eq!(text(&t), [["5", "151", "30.2", "7", "100"]]);
        let t = go("SELECT COUNT(*), SUM(sales) WHERE sales > 1000").unwrap();
        assert_eq!(text(&t), [["0", ""]], "no rows still give a row of counts");
    }

    #[test]
    fn count_of_a_column_skips_empty_ones() {
        assert_eq!(text(&go("SELECT COUNT(`unit price`), COUNT(*)").unwrap()), [["4", "5"]]);
    }

    #[test]
    fn min_and_max_of_words_go_by_the_alphabet() {
        assert_eq!(text(&go("SELECT MIN(name), MAX(name)").unwrap()), [["Ann", "Ed"]]);
    }

    #[test]
    fn group_by_makes_a_row_for_each_group_in_the_order_first_seen() {
        let t = go("SELECT region, COUNT(*), SUM(sales) GROUP BY region").unwrap();
        assert_eq!(text(&t), [["north", "2", "17"], ["south", "2", "125"], ["east", "1", "9"]]);
        let t = go("SELECT region, MAX(sales) AS best GROUP BY region ORDER BY best DESC LIMIT 1").unwrap();
        assert_eq!(text(&t), [["south", "100"]]);
        assert_eq!(t.columns[1].name, "best");
    }

    #[test]
    fn group_by_with_where_and_having() {
        let t = go("SELECT region, COUNT(*) WHERE sales > 8 GROUP BY region HAVING COUNT(*) > 1").unwrap();
        assert_eq!(text(&t), [["south", "2"]]);
    }

    #[test]
    fn a_column_outside_the_group_needs_an_aggregate() {
        let e = go("SELECT name, COUNT(*) GROUP BY region").unwrap_err();
        assert!(e.message.contains("name") && e.message.contains("GROUP BY"), "{}", e.message);
        assert_eq!(e.at, 7);
    }

    #[test]
    fn arithmetic_and_functions() {
        let t = go("SELECT sales * `unit price` AS total, UPPER(name), LENGTH(region), ROUND(sales / 3, 1) WHERE name = 'Ann'").unwrap();
        assert_eq!(text(&t), [["25", "ANN", "5", "3.3"]]);
        let t = go("SELECT sales + 1 * 2, -sales, (sales + 1) * 2, sales % 4 WHERE name = 'Ann'").unwrap();
        assert_eq!(text(&t), [["12", "-10", "22", "2"]]);
        assert_eq!(text(&go("SELECT name || '-' || region WHERE name = 'Bob'").unwrap()), [["Bob-south"]]);
    }

    #[test]
    fn an_empty_value_makes_a_sum_empty_not_a_mistake() {
        let t = go("SELECT sales * `unit price` WHERE name = 'Ed'").unwrap();
        assert_eq!(t.rows[0][0], Cell::Null);
    }

    #[test]
    fn distinct_drops_repeats() {
        assert_eq!(column(&go("SELECT DISTINCT region").unwrap(), 0), ["north", "south", "east"]);
    }

    #[test]
    fn headings_with_spaces_are_quoted_in_backticks_or_double_quotes() {
        assert_eq!(column(&go("SELECT `unit price` WHERE `unit price` > 2").unwrap(), 0), ["2.5", "4"]);
        let t = go("SELECT \"unit price\"").unwrap();
        assert_eq!(t.columns[0].name, "unit price");
        assert_eq!(column(&go("select NAME where SALES > 50").unwrap(), 0), ["Di"], "headings ignore case");
    }

    #[test]
    fn without_a_header_the_columns_are_a1_a2() {
        let rows: Vec<Vec<String>> = [["x", "3"], ["y", "1"], ["z", "2"]].iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect();
        let t = run("SELECT a1 ORDER BY a2", None, &rows).unwrap();
        assert_eq!(column(&t, 0), ["y", "z", "x"]);
        assert_eq!(t.columns[0].name, "a1");
        let e = run("SELECT a3", None, &rows).unwrap_err();
        assert!(e.message.contains("a3") && e.message.contains("a1"), "{}", e.message);
    }

    #[test]
    fn a1_works_beside_headings_and_a_heading_wins_a_clash() {
        assert_eq!(column(&go("SELECT a1 WHERE a3 = 7").unwrap(), 0), ["Cy"]);
        let head = vec!["a2".to_string(), "x".to_string()];
        let rows = vec![vec!["p".to_string(), "q".to_string()]];
        assert_eq!(column(&run("SELECT a2", Some(&head), &rows).unwrap(), 0), ["p"]);
    }

    #[test]
    fn short_rows_give_empty_values() {
        let rows = vec![vec!["a".to_string(), "b".to_string()], vec!["c".to_string()]];
        let t = run("SELECT a2", None, &rows).unwrap();
        assert_eq!(column(&t, 0), ["b", ""]);
    }

    #[test]
    fn text_in_single_quotes_can_hold_a_quote() {
        assert_eq!(go("SELECT name WHERE name = 'O''Neil'").unwrap().rows.len(), 0);
        assert_eq!(text(&go("SELECT 'it''s'  LIMIT 1").unwrap()), [["it's"]]);
    }

    #[test]
    fn from_names_the_file_and_is_ignored() {
        assert_eq!(go("SELECT name FROM data LIMIT 1").unwrap().rows.len(), 1);
    }

    #[test]
    fn mistakes_say_where_they_are() {
        let e = go("SELECT name WHERE sales >").unwrap_err();
        assert_eq!(e.at, 25);
        assert!(e.message.contains("the end of the query"), "{}", e.message);
        let e = go("SELECT name, WHERE sales > 1").unwrap_err();
        assert_eq!(e.at, 13, "{}", e.message);
        let e = go("SELECT nam").unwrap_err();
        assert_eq!(e.at, 7);
        assert!(e.message.contains("nam") && e.message.contains("name") && e.message.contains("region"), "{}", e.message);
        let e = go("name").unwrap_err();
        assert_eq!(e.at, 0);
        assert!(e.message.contains("SELECT"));
        let e = go("SELECT name ORDER sales").unwrap_err();
        assert_eq!((e.at, e.message.contains("BY")), (18, true));
        let e = go("SELECT name LIMIT ten").unwrap_err();
        assert_eq!(e.at, 18);
        let e = go("SELECT name WHERE (sales > 1").unwrap_err();
        assert_eq!(e.at, 28);
        let e = go("SELECT name WHERE name = 'Ann").unwrap_err();
        assert_eq!(e.at, 25);
        let e = go("SELECT name WHERE sales # 3").unwrap_err();
        assert_eq!(e.at, 24);
        let e = go("SELECT name extra").unwrap_err();
        assert_eq!(e.at, 12);
        assert_eq!(go("SELECT").unwrap_err().at, 6);
    }

    #[test]
    fn mistakes_over_several_lines_name_the_line() {
        let query = "SELECT name\nWHERE nope > 1";
        let e = go(query).unwrap_err();
        assert!(e.describe(query).contains("line 2, character 7"), "{}", e.describe(query));
        let query = "SELECT nope";
        assert!(go(query).unwrap_err().describe(query).ends_with("(character 8)"));
    }

    #[test]
    fn aggregates_are_kept_out_of_where_and_each_other() {
        let e = go("SELECT name WHERE SUM(sales) > 1").unwrap_err();
        assert_eq!(e.at, 18);
        assert!(e.message.contains("WHERE"));
        assert!(go("SELECT SUM(MAX(sales))").is_err());
        assert!(go("SELECT SUM(*)").is_err());
        assert!(go("SELECT *, COUNT(*)").is_err());
    }

    #[test]
    fn working_with_words_as_numbers_is_a_mistake_that_points_at_them() {
        let e = go("SELECT name + 1").unwrap_err();
        assert_eq!(e.at, 7);
        assert!(e.message.contains("Ann"), "{}", e.message);
        let e = go("SELECT SUM(name)").unwrap_err();
        assert!(e.message.contains("SUM"), "{}", e.message);
        let e = go("SELECT sales / 0").unwrap_err();
        assert!(e.message.contains("zero"));
        assert!(go("SELECT name WHERE name").is_err(), "text isn't a condition");
    }

    #[test]
    fn a_trailing_semicolon_is_fine() {
        assert_eq!(go("SELECT name LIMIT 1;").unwrap().rows.len(), 1);
    }

    #[test]
    fn money_and_percentages_count_as_numbers() {
        let rows: Vec<Vec<String>> = [["$5"], ["$12"], ["3%"]].iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect();
        assert_eq!(column(&run("SELECT a1 WHERE a1 > 4 ORDER BY a1 DESC", None, &rows).unwrap(), 0), ["$12", "$5"]);
        assert_eq!(text(&run("SELECT SUM(a1)", None, &rows).unwrap()), [["20"]]);
    }
}
