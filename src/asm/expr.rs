//! Expressions in assembler source: numbers written every way the old assemblers wrote them, characters, labels,
//! the address being assembled, and the usual operators, worked out as whole numbers.

/// What an expression needs from the assembler: labels' values, and where it's at.
pub trait Scope {
    /// A label's value; None if it isn't known (yet).
    fn value(&self, name: &str) -> Option<i64>;
    /// The address of the line being assembled.
    fn here(&self) -> i64;
    /// What a label written in the source is called in full: a local label belongs to the label before it.
    fn full_name(&self, name: &str) -> String;
    /// Whether ^ raises to a power, as in ACME, rather than being exclusive or.
    fn caret_is_power(&self) -> bool {
        false
    }
    /// A label was used: as written, and its full name. Kept on the last pass, for going to it and finding uses.
    fn used(&self, _written: &str, _full: &str) {}
    /// The memory page a label is in, under a virtual device ($$label in sjasmplus).
    fn page_of(&self, _name: &str) -> Option<i64> {
        None
    }
    /// The page being assembled into ($$).
    fn page_here(&self) -> Option<i64> {
        None
    }
    /// A byte or word of the device's memory ({address} and {b address}).
    fn peek(&self, _address: i64, _size: usize) -> Option<i64> {
        None
    }
    /// Where code is put, inside DISP, rather than the address it's assembled for ($$$).
    fn physical_here(&self) -> i64 {
        self.here()
    }
    /// A label's physical address ($$$label): where its code was put.
    fn physical_of(&self, name: &str) -> Option<i64> {
        self.value(name)
    }
}

/// What came of working one out.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Value {
    Known(i64),
    /// It names a label not known yet: fine on an early pass, a problem on the last.
    Unknown,
}

impl Value {
    fn map2(self, other: Value, f: impl FnOnce(i64, i64) -> Result<i64, String>) -> Result<Value, String> {
        match (self, other) {
            (Value::Known(a), Value::Known(b)) => f(a, b).map(Value::Known),
            _ => Ok(Value::Unknown),
        }
    }
}

/// Works out an expression; an error says what's wrong with it.
pub fn eval(text: &str, scope: &dyn Scope) -> Result<Value, String> {
    let mut parser = Parser { text: text.as_bytes(), at: 0, scope, unknown: None };
    let value = parser.binary(0)?;
    parser.space();
    if parser.at < parser.text.len() {
        return Err(format!("\"{}\" wasn't understood", text[parser.at..].trim()));
    }
    Ok(value)
}

/// The labels an expression names that aren't known, for messages.
pub fn first_unknown(text: &str, scope: &dyn Scope) -> Option<String> {
    let mut parser = Parser { text: text.as_bytes(), at: 0, scope, unknown: None };
    let _ = parser.binary(0);
    parser.unknown
}

struct Parser<'a> {
    text: &'a [u8],
    at: usize,
    scope: &'a dyn Scope,
    unknown: Option<String>,
}

/// Binary operators, loosest first.
const OPERATORS: [(&str, u8); 19] = [
    ("||", 1),
    ("&&", 2),
    ("|", 3),
    ("^", 4),
    ("&", 5),
    ("==", 6),
    ("!=", 6),
    ("<>", 6),
    ("<=", 7),
    (">=", 7),
    ("<<", 8),
    (">>", 8),
    ("<", 7),
    (">", 7),
    ("+", 9),
    ("-", 9),
    ("*", 10),
    ("/", 10),
    // ACME compares with a single =.
    ("=", 6),
];

impl Parser<'_> {
    fn space(&mut self) {
        while self.at < self.text.len() && self.text[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn rest(&self) -> &[u8] {
        &self.text[self.at..]
    }

    fn operator(&mut self) -> Option<(&'static str, u8)> {
        self.space();
        let rest = self.rest();
        // Words some assemblers use: MOD, AND, OR, XOR, SHL, SHR.
        for (word, op, level) in [("mod", "%", 10u8), ("and", "&", 5), ("or", "|", 3), ("xor", "^", 4), ("shl", "<<", 8), ("shr", ">>", 8)] {
            if rest.len() > word.len() && rest[..word.len()].eq_ignore_ascii_case(word.as_bytes()) && !is_name_char(rest[word.len()]) && !is_name_char(*self.text.get(self.at.wrapping_sub(1)).unwrap_or(&b' ')) {
                return Some((op, level));
            }
        }
        // After a value, % is the remainder, never a binary number.
        if rest.first() == Some(&b'%') {
            return Some(("%", 10));
        }
        if rest.first() == Some(&b'^') && self.scope.caret_is_power() {
            return Some(("**", 11));
        }
        OPERATORS.iter().find(|(op, _)| rest.starts_with(op.as_bytes())).copied()
    }

    fn binary(&mut self, least: u8) -> Result<Value, String> {
        let mut left = self.unary()?;
        loop {
            let at = self.at;
            let Some((op, level)) = self.operator() else {
                break;
            };
            if level <= least {
                self.at = at;
                break;
            }
            // A word operator is as long as its word; a symbol as long as itself.
            let rest = self.rest();
            let length = if rest.first().is_some_and(u8::is_ascii_alphabetic) {
                rest.iter().take_while(|c| c.is_ascii_alphabetic()).count()
            } else if op == "**" {
                1
            } else {
                op.len()
            };
            self.at += length;
            let right = self.binary(level)?;
            left = left.map2(right, |a, b| apply(op, a, b))?;
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Value, String> {
        self.space();
        match self.peek() {
            Some(b'-') => {
                self.at += 1;
                Ok(match self.unary()? {
                    Value::Known(v) => Value::Known(v.wrapping_neg()),
                    unknown => unknown,
                })
            }
            Some(b'+') => {
                self.at += 1;
                self.unary()
            }
            Some(b'~') => {
                self.at += 1;
                Ok(match self.unary()? {
                    Value::Known(v) => Value::Known(!v),
                    unknown => unknown,
                })
            }
            Some(b'!') => {
                self.at += 1;
                Ok(match self.unary()? {
                    Value::Known(v) => Value::Known((v == 0) as i64),
                    unknown => unknown,
                })
            }
            // The low or high byte, as 6502 assemblers write it.
            Some(b'<') => {
                self.at += 1;
                Ok(match self.unary()? {
                    Value::Known(v) => Value::Known(v & 0xFF),
                    unknown => unknown,
                })
            }
            Some(b'>') => {
                self.at += 1;
                Ok(match self.unary()? {
                    Value::Known(v) => Value::Known((v >> 8) & 0xFF),
                    unknown => unknown,
                })
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Result<Value, String> {
        self.space();
        let Some(first) = self.peek() else {
            return Err("a value is missing".into());
        };
        match first {
            b'(' | b'[' => {
                let close = if first == b'(' { b')' } else { b']' };
                self.at += 1;
                let value = self.binary(0)?;
                self.space();
                if self.peek() != Some(close) {
                    return Err(format!("a \"{}\" is missing", close as char));
                }
                self.at += 1;
                Ok(value)
            }
            b'\'' | b'"' => {
                // A character, or two packed as a word.
                let quote = first;
                let start = self.at + 1;
                let Some(length) = self.text[start..].iter().position(|c| *c == quote) else {
                    return Err("a quote isn't closed".into());
                };
                let bytes = &self.text[start..start + length];
                self.at = start + length + 1;
                match bytes {
                    [] => Err("an empty character".into()),
                    // Up to four characters, packed first-is-highest, as sjasmplus has them.
                    _ if bytes.len() <= 4 => Ok(Value::Known(bytes.iter().fold(0i64, |v, b| (v << 8) | *b as i64))),
                    _ => Err("a character value can only be one to four characters".into()),
                }
            }
            // sjasmplus: $$$ is the address code is put at (inside DISP), and $$$label a label's.
            b'$' if self.rest().starts_with(b"$$$") && !self.rest().starts_with(b"$$$$") => {
                self.at += 3;
                if self.peek().is_some_and(|c| is_name_start(c) || c == b'.') {
                    let start = self.at;
                    while self.peek().is_some_and(is_name_char) {
                        self.at += 1;
                    }
                    let name = std::str::from_utf8(&self.text[start..self.at]).unwrap_or_default().to_string();
                    let full = self.scope.full_name(&name);
                    return Ok(match self.scope.physical_of(&full) {
                        Some(v) => Value::Known(v),
                        None => {
                            self.unknown.get_or_insert(name);
                            Value::Unknown
                        }
                    });
                }
                Ok(Value::Known(self.scope.physical_here()))
            }
            // sjasmplus: $$label is the page a label is in, and $$ the page being assembled into.
            b'$' if self.rest().get(1) == Some(&b'$') => {
                self.at += 2;
                if self.peek().is_some_and(|c| is_name_start(c) || c == b'@' || c == b'.') {
                    let start = self.at;
                    while self.peek().is_some_and(|c| is_name_char(c) || c == b'@') {
                        self.at += 1;
                    }
                    let name = std::str::from_utf8(&self.text[start..self.at]).unwrap_or_default().to_string();
                    let full = self.scope.full_name(&name);
                    self.scope.used(&name, &full);
                    return Ok(match self.scope.page_of(&full) {
                        Some(page) => Value::Known(page),
                        None => {
                            self.unknown.get_or_insert(name);
                            Value::Unknown
                        }
                    });
                }
                self.scope.page_here().map(Value::Known).ok_or_else(|| "$$ is the page being assembled into, which needs a DEVICE".into())
            }
            // sjasmplus: {address} reads a word of the device's memory, {b address} a byte.
            b'{' => {
                self.at += 1;
                self.space();
                let rest = self.rest();
                let size = if rest.len() > 1 && (rest[0] | 0x20) == b'b' && rest[1] == b' ' {
                    self.at += 2;
                    1
                } else {
                    2
                };
                let address = self.binary(0)?;
                self.space();
                if self.peek() != Some(b'}') {
                    return Err("a \"}\" is missing".into());
                }
                self.at += 1;
                Ok(match address {
                    Value::Known(a) => Value::Known(self.scope.peek(a, size).ok_or("{address} reads the memory of a DEVICE, and there's none")?),
                    unknown => unknown,
                })
            }
            b'$' | b'#' | b'&' if self.rest().get(1).is_some_and(u8::is_ascii_hexdigit) => {
                self.at += 1;
                self.digits(16)
            }
            b'$' | b'*' => {
                // Where it's at.
                self.at += 1;
                Ok(Value::Known(self.scope.here()))
            }
            b'%' if self.rest().get(1).is_some_and(|c| matches!(c, b'0' | b'1' | b'.' | b'#')) => {
                // ACME writes binary with . for 0 and # for 1 too, to draw with.
                self.at += 1;
                let start = self.at;
                // sjasmplus separates digits with ', as C++ does.
                while self.peek().is_some_and(|c| matches!(c, b'0' | b'1' | b'.' | b'#' | b'_') || (c == b'\'' && self.text.get(self.at + 1).is_some_and(|n| matches!(n, b'0' | b'1')))) {
                    self.at += 1;
                }
                let bits: String = self.text[start..self.at].iter().filter(|c| **c != b'_' && **c != b'\'').map(|c| if matches!(c, b'1' | b'#') { '1' } else { '0' }).collect();
                i64::from_str_radix(&bits, 2).map(Value::Known).map_err(|_| "a binary number is missing its digits".into())
            }
            b'0'..=b'9' => self.number(),
            c if is_name_start(c) => {
                let start = self.at;
                while self.peek().is_some_and(is_name_char) {
                    self.at += 1;
                }
                let name = std::str::from_utf8(&self.text[start..self.at]).unwrap_or_default().to_string();
                // A function: high(x), low(x).
                self.space();
                // high(x), or as sjasmplus also has it, high x.
                let word_follows = self.peek().is_some_and(|c| is_name_start(c) || c.is_ascii_digit() || matches!(c, b'$' | b'#' | b'%' | b'('));
                if word_follows && matches!(name.to_ascii_lowercase().as_str(), "high" | "low" | "hi" | "lo") {
                    let value = self.unary()?;
                    return Ok(match value {
                        Value::Known(v) if name.to_ascii_lowercase().starts_with('h') => Value::Known((v >> 8) & 0xFF),
                        Value::Known(v) => Value::Known(v & 0xFF),
                        unknown => unknown,
                    });
                }
                let full = self.scope.full_name(&name);
                self.scope.used(&name, &full);
                match self.scope.value(&full) {
                    Some(value) => Ok(Value::Known(value)),
                    None => {
                        self.unknown.get_or_insert(name);
                        Ok(Value::Unknown)
                    }
                }
            }
            _ => Err(format!("\"{}\" wasn't understood", String::from_utf8_lossy(self.rest()).trim())),
        }
    }

    fn digits(&mut self, radix: u32) -> Result<Value, String> {
        let start = self.at;
        let separator = |parser: &Self, c: u8| c == b'\'' && parser.text.get(parser.at + 1).is_some_and(|n| (*n as char).is_digit(radix)) && parser.at > start;
        while self.peek().is_some_and(|c| (c as char).is_digit(radix) || c == b'_' || separator(self, c)) {
            self.at += 1;
        }
        let text: String = std::str::from_utf8(&self.text[start..self.at]).unwrap_or_default().chars().filter(|c| *c != '_' && *c != '\'').collect();
        i64::from_str_radix(&text, radix).map(Value::Known).map_err(|_| "a number is missing its digits".into())
    }

    /// A number starting with a digit: decimal, 0x hex, 0b binary, or with an H, B, O or Q after it.
    fn number(&mut self) -> Result<Value, String> {
        let rest = self.rest();
        if rest.len() > 2 && rest[0] == b'0' && (rest[1] | 0x20) == b'x' && rest[2].is_ascii_hexdigit() {
            self.at += 2;
            return self.digits(16);
        }
        if rest.len() > 2 && rest[0] == b'0' && (rest[1] | 0x20) == b'b' && (rest[2] == b'0' || rest[2] == b'1') && !rest[2..].iter().take_while(|c| c.is_ascii_alphanumeric()).any(|c| (c | 0x20) == b'h') {
            self.at += 2;
            return self.digits(2);
        }
        // A ' between digits separates them, as sjasmplus allows.
        let mut word: Vec<u8> = Vec::new();
        for (i, c) in rest.iter().copied().enumerate() {
            let separator = c == b'\'' && i > 0 && rest.get(i + 1).is_some_and(u8::is_ascii_alphanumeric);
            if !(c.is_ascii_alphanumeric() || c == b'_' || separator) {
                break;
            }
            word.push(c);
        }
        let digits: String = word.iter().map(|c| *c as char).filter(|c| *c != '_' && *c != '\'').collect();
        let (body, radix) = match digits.chars().last().map(|c| c.to_ascii_lowercase()) {
            Some('h') => (&digits[..digits.len() - 1], 16),
            Some('b') if digits[..digits.len() - 1].chars().all(|c| c == '0' || c == '1') => (&digits[..digits.len() - 1], 2),
            Some('o' | 'q') => (&digits[..digits.len() - 1], 8),
            Some('d') if digits[..digits.len() - 1].chars().all(|c| c.is_ascii_digit()) => (&digits[..digits.len() - 1], 10),
            _ => (&digits[..], 10),
        };
        let value = i64::from_str_radix(body, radix).map_err(|_| format!("\"{digits}\" isn't a number"))?;
        self.at += word.len();
        Ok(Value::Known(value))
    }
}

fn apply(op: &str, a: i64, b: i64) -> Result<i64, String> {
    Ok(match op {
        "||" => (a != 0 || b != 0) as i64,
        "&&" => (a != 0 && b != 0) as i64,
        "|" => a | b,
        "^" => a ^ b,
        "&" => a & b,
        "==" | "=" => (a == b) as i64,
        "!=" | "<>" => (a != b) as i64,
        "<=" => (a <= b) as i64,
        ">=" => (a >= b) as i64,
        "<" => (a < b) as i64,
        ">" => (a > b) as i64,
        "<<" => a.wrapping_shl(b as u32),
        ">>" => a.wrapping_shr(b as u32),
        "+" => a.wrapping_add(b),
        "-" => a.wrapping_sub(b),
        "*" => a.wrapping_mul(b),
        "/" if b == 0 => return Err("division by zero".into()),
        "/" => a / b,
        "%" if b == 0 => return Err("division by zero".into()),
        "%" => a % b,
        "**" if b < 0 => 0,
        "**" => a.wrapping_pow(b.min(63) as u32),
        _ => return Err(format!("{op} isn't an operator")),
    })
}

pub fn is_name_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'.' || c == b'@' || c == b'?'
}

/// A character a name can go on with: sjasmplus allows # in names too (as in ld_bc_#hl#).
pub fn is_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'.' || c == b'@' || c == b'?' || c == b'#'
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Labels;
    impl Scope for Labels {
        fn value(&self, name: &str) -> Option<i64> {
            match name {
                "start" => Some(0x8000),
                "main.loop" => Some(0x8010),
                _ => None,
            }
        }
        fn here(&self) -> i64 {
            0x9000
        }
        fn full_name(&self, name: &str) -> String {
            if name.starts_with('.') { format!("main{name}") } else { name.to_string() }
        }
    }

    fn value(text: &str) -> Value {
        eval(text, &Labels).unwrap()
    }

    #[test]
    fn numbers_are_read_every_way_they_were_written() {
        for (text, expected) in [("255", 255), ("$FF", 255), ("#ff", 255), ("0xFF", 255), ("0FFh", 255), ("%11111111", 255), ("0b1111_1111", 255), ("11111111b", 255), ("377o", 255), ("377q", 255), ("&FF", 255), ("'A'", 65), ("\"AB\"", 0x4142), ("%00000'0'0'1", 1), ("$12'34", 0x1234), ("1'000", 1000), ("\"0004\"", 0x3030_3034)] {
            assert_eq!(value(text), Value::Known(expected), "{text}");
        }
    }

    #[test]
    fn operators_work_in_the_usual_order() {
        assert_eq!(value("2+3*4"), Value::Known(14));
        assert_eq!(value("(2+3)*4"), Value::Known(20));
        assert_eq!(value("1<<4|1"), Value::Known(17));
        assert_eq!(value("-1 & $FF"), Value::Known(255));
        assert_eq!(value("17 % 5"), Value::Known(2));
        assert_eq!(value("17 mod 5"), Value::Known(2));
        assert_eq!(value("high(start)+low($1234)"), Value::Known(0x80 + 0x34));
        assert_eq!(value("low start"), Value::Known(0));
        assert_eq!(value("high start + 1"), Value::Known(0x81));
        assert_eq!(value("<$1234"), Value::Known(0x34));
        assert_eq!(value(">$1234"), Value::Known(0x12));
        assert_eq!(value("3 > 2"), Value::Known(1));
        assert_eq!(value("3 = 3"), Value::Known(1));
        assert_eq!(value("%..##..#."), Value::Known(0b0011_0010));
        assert_eq!(value("$+2"), Value::Known(0x9002));
        assert_eq!(value("*-start"), Value::Known(0x1000));
        assert_eq!(value(".loop"), Value::Known(0x8010));
    }

    #[test]
    fn labels_not_known_yet_are_unknown_and_named() {
        assert_eq!(value("later+1"), Value::Unknown);
        assert_eq!(first_unknown("start+later", &Labels).as_deref(), Some("later"));
        assert!(eval("1/0", &Labels).is_err());
        assert!(eval("2 +", &Labels).is_err());
        assert!(eval("(2", &Labels).is_err());
    }
}
