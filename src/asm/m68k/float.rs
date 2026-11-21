//! Floating-point numbers in the 68881's and 68882's formats: single (.S), double (.D) and extended (.X)
//! precision, and packed decimal (.P), from what the source writes and back again.
//!
//! A number written in decimal is made exactly, rounded once to the nearest the format holds (ties to even), as
//! the FPU itself would round it. vasm makes .S and .D the same way, but it goes through double precision for .X
//! and .P, so a few constants come out differently from vasm's: those that need more than double precision's 53
//! bits (dc.x 0.1, say), and packed decimals, which vasm writes from the nearest double rather than from the digits
//! the source gave (dc.p 0.1 would be 1.0000000000000001E-1). Everything else is vasm's way: an expression that
//! starts with a decimal number is worked out in floating point, and any other (a hexadecimal number, a label) is
//! the bits themselves, so #$3FC00000 is 1.5 in single precision.
//!
//! Going back, a number is written in decimal only when both Limoniter's assembler and vasm would make the same
//! bits from what's written; otherwise (NaNs, infinities, denormals, and digits vasm would round differently) it's
//! written as its bits in hexadecimal.

/// The FPU's formats, as their sizes are written (.B to .P) and as the command word's source specifier codes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Long,
    Single,
    Extended,
    Packed,
    Word,
    Double,
    Byte,
}

impl Format {
    pub const ALL: [Format; 7] = [Format::Long, Format::Single, Format::Extended, Format::Packed, Format::Word, Format::Double, Format::Byte];

    /// The command word's source specifier for it.
    pub fn code(self) -> u16 {
        match self {
            Format::Long => 0,
            Format::Single => 1,
            Format::Extended => 2,
            Format::Packed => 3,
            Format::Word => 4,
            Format::Double => 5,
            Format::Byte => 6,
        }
    }

    pub fn from_code(code: u16) -> Option<Format> {
        Format::ALL.into_iter().find(|f| f.code() == code)
    }

    pub fn bytes(self) -> usize {
        match self {
            Format::Byte => 1,
            Format::Word => 2,
            Format::Long | Format::Single => 4,
            Format::Double => 8,
            Format::Extended | Format::Packed => 12,
        }
    }

    /// The letter it's written with.
    pub fn letter(self) -> char {
        match self {
            Format::Long => 'l',
            Format::Single => 's',
            Format::Extended => 'x',
            Format::Packed => 'p',
            Format::Word => 'w',
            Format::Double => 'd',
            Format::Byte => 'b',
        }
    }

    pub fn from_letter(letter: &str) -> Option<Format> {
        Format::ALL.into_iter().find(|f| letter.len() == 1 && letter.starts_with(f.letter()))
    }

    /// Whether it's one of the floating-point formats rather than a whole number.
    pub fn is_float(self) -> bool {
        matches!(self, Format::Single | Format::Double | Format::Extended | Format::Packed)
    }
}

// ---------- Big whole numbers, for exact conversions ----------

/// A whole number of any size, its 32-bit pieces lowest first.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Big(Vec<u32>);

impl Big {
    fn from_u128(mut v: u128) -> Big {
        let mut pieces = Vec::new();
        while v != 0 {
            pieces.push(v as u32);
            v >>= 32;
        }
        Big(pieces)
    }

    fn trim(&mut self) {
        while self.0.last() == Some(&0) {
            self.0.pop();
        }
    }

    fn bits(&self) -> u32 {
        match self.0.iter().rposition(|p| *p != 0) {
            Some(top) => top as u32 * 32 + (32 - self.0[top].leading_zeros()),
            None => 0,
        }
    }

    fn mul_add(&mut self, by: u32, add: u32) {
        let mut carry = u64::from(add);
        for piece in &mut self.0 {
            let v = u64::from(*piece) * u64::from(by) + carry;
            *piece = v as u32;
            carry = v >> 32;
        }
        if carry != 0 {
            self.0.push(carry as u32);
        }
    }

    fn times_ten_to(&mut self, mut n: u32) {
        while n >= 9 {
            self.mul_add(1_000_000_000, 0);
            n -= 9;
        }
        if n > 0 {
            self.mul_add(10u32.pow(n), 0);
        }
    }

    fn shifted(&self, by: u32) -> Big {
        let (whole, part) = ((by / 32) as usize, by % 32);
        let mut pieces = vec![0u32; whole];
        let mut carry = 0u32;
        for piece in &self.0 {
            if part == 0 {
                pieces.push(*piece);
            } else {
                pieces.push(piece << part | carry);
                carry = piece >> (32 - part);
            }
        }
        if carry != 0 {
            pieces.push(carry);
        }
        let mut big = Big(pieces);
        big.trim();
        big
    }

    fn cmp(&self, other: &Big) -> std::cmp::Ordering {
        let (a, b) = (self.bits(), other.bits());
        if a != b {
            return a.cmp(&b);
        }
        let n = self.0.len().max(other.0.len());
        for i in (0..n).rev() {
            let (x, y) = (self.0.get(i).copied().unwrap_or(0), other.0.get(i).copied().unwrap_or(0));
            if x != y {
                return x.cmp(&y);
            }
        }
        std::cmp::Ordering::Equal
    }

    /// Takes `other` away; it mustn't be bigger.
    fn sub(&mut self, other: &Big) {
        let mut borrow = 0i64;
        for i in 0..self.0.len() {
            let v = i64::from(self.0[i]) - i64::from(other.0.get(i).copied().unwrap_or(0)) - borrow;
            borrow = i64::from(v < 0);
            self.0[i] = v.rem_euclid(1 << 32) as u32;
        }
        self.trim();
    }

    /// Divides by `by`, for a quotient below 2^bits: the quotient, and whether the remainder is below, at or above
    /// half of `by` (for rounding).
    fn divide(mut self, by: &Big, bits: u32) -> (u128, std::cmp::Ordering) {
        let mut quotient = 0u128;
        for i in (0..bits).rev() {
            let step = by.shifted(i);
            if self.cmp(&step) != std::cmp::Ordering::Less {
                self.sub(&step);
                quotient |= 1 << i;
            }
        }
        (quotient, self.shifted(1).cmp(by))
    }
}

// ---------- Decimal numbers ----------

/// A number as written in decimal: `digits` (with no zeros in front; none at all for zero) times ten to `exponent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decimal {
    pub negative: bool,
    pub digits: Vec<u8>,
    pub exponent: i32,
}

impl Decimal {
    /// A decimal number as vasm takes one: digits, perhaps a point and more digits, perhaps an exponent (1.5,
    /// 10., 2.5e-3, 1E+2). No sign: a minus in front is the expression's.
    pub fn parse(text: &str) -> Option<Decimal> {
        let b = text.as_bytes();
        let mut at = 0;
        let mut digits = Vec::new();
        let mut exponent = 0i32;
        if !b.first()?.is_ascii_digit() {
            return None;
        }
        while at < b.len() && b[at].is_ascii_digit() {
            digits.push(b[at] - b'0');
            at += 1;
        }
        if at < b.len() && b[at] == b'.' {
            at += 1;
            while at < b.len() && b[at].is_ascii_digit() {
                digits.push(b[at] - b'0');
                exponent -= 1;
                at += 1;
            }
        }
        if at < b.len() && (b[at] | 0x20) == b'e' {
            at += 1;
            let negative = match b.get(at) {
                Some(b'-') => {
                    at += 1;
                    true
                }
                Some(b'+') => {
                    at += 1;
                    false
                }
                _ => false,
            };
            let start = at;
            let mut e: i64 = 0;
            while at < b.len() && b[at].is_ascii_digit() {
                e = (e * 10 + i64::from(b[at] - b'0')).min(1_000_000);
                at += 1;
            }
            if at == start {
                return None;
            }
            exponent = (i64::from(exponent) + if negative { -e } else { e }) as i32;
        }
        if at != b.len() {
            return None;
        }
        let first = digits.iter().position(|d| *d != 0).unwrap_or(digits.len());
        digits.drain(..first);
        // Zeros at the end only make the number bigger to carry.
        while digits.last() == Some(&0) {
            digits.pop();
            exponent += 1;
        }
        Some(Decimal { negative: false, digits, exponent })
    }

    /// The shortest decimal that's exactly a double, as Rust writes it.
    fn of_f64(v: f64) -> Decimal {
        let text = format!("{:e}", v.abs());
        let mut d = Decimal::parse(&text).unwrap_or(Decimal { negative: false, digits: Vec::new(), exponent: 0 });
        d.negative = v.is_sign_negative();
        d
    }

    /// Where its first digit is, as a power of ten.
    fn magnitude(&self) -> i64 {
        i64::from(self.exponent) + self.digits.len() as i64 - 1
    }
}

/// A binary number in a format: sign, the exponent as it's kept (biased), and the mantissa with its integer bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Binary {
    negative: bool,
    exponent: i32,
    mantissa: u64,
}

/// How a binary format keeps its numbers: its precision (with the integer bit), and its exponents.
#[derive(Clone, Copy)]
struct Shape {
    precision: u32,
    bias: i32,
    /// The largest exponent field a finite number has.
    top: i32,
    /// Whether the integer bit is kept, as extended precision keeps it. Then (in Motorola's extended format) an
    /// exponent field of 0 is simply the lowest exponent, 2^-16383 for the integer bit, where single and double
    /// precision's 0 is the same as their 1 but with no integer bit.
    explicit: bool,
}

const SINGLE: Shape = Shape { precision: 24, bias: 127, top: 254, explicit: false };
const DOUBLE: Shape = Shape { precision: 53, bias: 1023, top: 2046, explicit: false };
const EXTENDED: Shape = Shape { precision: 64, bias: 16383, top: 32766, explicit: true };

/// Rounds a decimal to a binary format, exactly, ties to even: None if it's too big for it.
fn round_decimal(d: &Decimal, shape: Shape) -> Option<Binary> {
    let zero = Binary { negative: d.negative, exponent: 0, mantissa: 0 };
    if d.digits.is_empty() || d.magnitude() < -5000 {
        return Some(zero);
    }
    if d.magnitude() > 5000 {
        return None;
    }
    let mut n = Big(Vec::new());
    for digit in &d.digits {
        n.mul_add(10, u32::from(*digit));
    }
    let mut m = Big::from_u128(1);
    if d.exponent >= 0 {
        n.times_ten_to(d.exponent as u32);
    } else {
        m.times_ten_to(d.exponent.unsigned_abs());
    }
    let p = shape.precision;
    // The value is n / m. Its lowest bit's power of two, for a quotient of p bits (or one more).
    let mut e = n.bits() as i32 - m.bits() as i32 - p as i32;
    let lowest = if shape.explicit { 0 } else { 1 } - shape.bias - (p as i32 - 1);
    let quotient = |e: i32| -> (u128, std::cmp::Ordering) {
        if e >= 0 { n.clone().divide(&m.shifted(e as u32), p + 2) } else { n.shifted(e.unsigned_abs()).divide(&m, p + 2) }
    };
    let (mut q, mut rest) = quotient(e);
    if q >> p != 0 {
        e += 1;
        (q, rest) = quotient(e);
    }
    // Too small for the format's smallest exponent: a denormal, with fewer bits.
    if e < lowest {
        e = lowest;
        (q, rest) = quotient(e);
    }
    let up = rest == std::cmp::Ordering::Greater || (rest == std::cmp::Ordering::Equal && q & 1 == 1);
    if up {
        q += 1;
        if q >> p != 0 {
            q >>= 1;
            e += 1;
        }
    }
    let exponent = if shape.explicit || q >> (p - 1) != 0 { e + p as i32 - 1 + shape.bias } else { 0 };
    if exponent > shape.top {
        return None;
    }
    Some(Binary { negative: d.negative, exponent, mantissa: q as u64 })
}

/// A binary number rounded again to a narrower format, ties to even, as vasm rounds what it reads in extended
/// precision to single or double: None if it's too big.
fn round_again(b: Binary, from: Shape, to: Shape) -> Option<Binary> {
    if b.mantissa == 0 {
        return Some(Binary { exponent: 0, ..b });
    }
    // Its value is mantissa * 2^(e - (precision - 1)), the exponent unbiased.
    let e = if b.exponent == 0 && !from.explicit { 1 - from.bias } else { b.exponent - from.bias };
    let lead = 63 - b.mantissa.leading_zeros() as i32 - (from.precision as i32 - 1) + e;
    let wanted_lowest = (lead - (to.precision as i32 - 1)).max(1 - to.bias - (to.precision as i32 - 1));
    let have_lowest = e - (from.precision as i32 - 1);
    let drop = wanted_lowest - have_lowest;
    let mut q = u128::from(b.mantissa);
    let mut lowest = have_lowest;
    if drop > 0 {
        let drop = drop.min(100) as u32;
        let kept = if drop >= 128 { 0 } else { q >> drop };
        let rest = q - (kept << drop.min(127));
        let half = 1u128 << (drop - 1).min(127);
        q = kept;
        if rest > half || (rest == half && q & 1 == 1) {
            q += 1;
        }
        lowest = wanted_lowest;
    } else if drop < 0 {
        q <<= (-drop) as u32;
        lowest = wanted_lowest;
    }
    if q >> to.precision != 0 {
        q >>= 1;
        lowest += 1;
    }
    let exponent = if q >> (to.precision - 1) != 0 { lowest + to.precision as i32 - 1 + to.bias } else { 0 };
    if exponent > to.top {
        return None;
    }
    Some(Binary { negative: b.negative, exponent, mantissa: q as u64 })
}

fn single_bits(b: Binary) -> u32 {
    (b.negative as u32) << 31 | (b.exponent as u32) << 23 | (b.mantissa as u32 & 0x7F_FFFF)
}

fn double_bits(b: Binary) -> u64 {
    (b.negative as u64) << 63 | (b.exponent as u64) << 52 | (b.mantissa & 0xF_FFFF_FFFF_FFFF)
}

fn extended_bytes(b: Binary) -> Vec<u8> {
    let mut bytes = ((b.negative as u16) << 15 | b.exponent as u16).to_be_bytes().to_vec();
    bytes.extend([0, 0]);
    bytes.extend(b.mantissa.to_be_bytes());
    bytes
}

/// A double, as extended precision holds it exactly.
fn extended_of_f64(v: f64) -> Vec<u8> {
    let bits = v.to_bits();
    let negative = bits >> 63 != 0;
    let field = (bits >> 52 & 0x7FF) as i32;
    let fraction = bits & 0xF_FFFF_FFFF_FFFF;
    let b = match field {
        0 if fraction == 0 => Binary { negative, exponent: 0, mantissa: 0 },
        0 => {
            // A double's denormal is a normal extended number.
            let shift = fraction.leading_zeros();
            Binary { negative, exponent: 16383 - 1022 - (shift as i32 - 11), mantissa: fraction << shift }
        }
        0x7FF => Binary { negative, exponent: 0x7FFF, mantissa: if fraction == 0 { 0 } else { 1 << 63 | fraction << 11 } },
        _ => Binary { negative, exponent: field - 1023 + 16383, mantissa: 1 << 63 | fraction << 11 },
    };
    extended_bytes(b)
}

/// Packed decimal: the sign of the number and of its exponent, three digits of exponent, one digit before the
/// point and sixteen after, each four bits. None if the exponent is past 999 either way.
fn packed_bytes(d: &Decimal) -> Option<Vec<u8>> {
    let mut bytes = vec![0u8; 12];
    if d.digits.is_empty() {
        bytes[0] = (d.negative as u8) << 7;
        return Some(bytes);
    }
    let mut digits = d.digits.clone();
    let mut magnitude = d.magnitude();
    if digits.len() > 17 {
        let rest = digits.split_off(17);
        let half = rest[0].cmp(&5).then(if rest[1..].iter().any(|r| *r != 0) { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Equal });
        if half == std::cmp::Ordering::Greater || (half == std::cmp::Ordering::Equal && digits[16] & 1 == 1) {
            let mut at = 16;
            loop {
                if digits[at] < 9 {
                    digits[at] += 1;
                    break;
                }
                digits[at] = 0;
                if at == 0 {
                    digits.insert(0, 1);
                    digits.truncate(17);
                    magnitude += 1;
                    break;
                }
                at -= 1;
            }
        }
    }
    digits.resize(17, 0);
    if magnitude.abs() > 999 {
        return None;
    }
    let e = magnitude.unsigned_abs() as u32;
    bytes[0] = (d.negative as u8) << 7 | ((magnitude < 0) as u8) << 6 | (e / 100 % 10) as u8;
    bytes[1] = (((e / 10 % 10) << 4) | (e % 10)) as u8;
    bytes[3] = digits[0];
    for (i, pair) in digits[1..].chunks(2).enumerate() {
        bytes[4 + i] = pair[0] << 4 | pair[1];
    }
    Some(bytes)
}

// ---------- From the source ----------

/// A value worked out in floating point: exactly as written, for a lone number, or the double an expression came to.
#[derive(Clone, Debug, PartialEq)]
enum Value {
    Exact(Decimal),
    Approximate(f64),
}

impl Value {
    fn f64(&self) -> f64 {
        match self {
            Value::Approximate(v) => *v,
            Value::Exact(d) => {
                let text = format!("{}{}e{}", if d.negative { "-" } else { "" }, if d.digits.is_empty() { "0".to_string() } else { d.digits.iter().map(|d| char::from(b'0' + d)).collect() }, d.exponent);
                text.parse().unwrap_or(0.0)
            }
        }
    }

    fn negated(self) -> Value {
        match self {
            Value::Exact(mut d) => {
                d.negative = !d.negative;
                Value::Exact(d)
            }
            Value::Approximate(v) => Value::Approximate(-v),
        }
    }
}

/// An expression in floating point, as vasm takes one that starts with a decimal number: + - * / and brackets,
/// with decimal numbers, and whole numbers and labels for their values.
struct Parser<'a> {
    text: &'a [u8],
    at: usize,
    eval: &'a mut dyn FnMut(&str) -> Result<i64, String>,
}

impl Parser<'_> {
    fn skip(&mut self) {
        while self.at < self.text.len() && self.text[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.text.get(self.at).copied()
    }

    fn sum(&mut self) -> Result<Value, String> {
        let mut v = self.product()?;
        while let Some(c @ (b'+' | b'-')) = self.peek() {
            self.at += 1;
            let w = self.product()?.f64();
            v = Value::Approximate(if c == b'+' { v.f64() + w } else { v.f64() - w });
        }
        Ok(v)
    }

    fn product(&mut self) -> Result<Value, String> {
        let mut v = self.unary()?;
        while let Some(c @ (b'*' | b'/')) = self.peek() {
            self.at += 1;
            let w = self.unary()?.f64();
            v = Value::Approximate(if c == b'*' { v.f64() * w } else { v.f64() / w });
        }
        Ok(v)
    }

    fn unary(&mut self) -> Result<Value, String> {
        match self.peek() {
            Some(b'-') => {
                self.at += 1;
                Ok(self.unary()?.negated())
            }
            Some(b'+') => {
                self.at += 1;
                self.unary()
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Value, String> {
        let wrong = || crate::i18n::tr("this isn't a floating-point expression Limoniter can work out (it takes numbers, labels, + - * / and brackets)");
        match self.peek() {
            Some(b'(') => {
                self.at += 1;
                let v = self.sum()?;
                if self.peek() != Some(b')') {
                    return Err(wrong().into());
                }
                self.at += 1;
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() && !(c == b'0' && self.text.get(self.at + 1).is_some_and(|x| x | 0x20 == b'x')) => {
                let start = self.at;
                while self.at < self.text.len() && (self.text[self.at].is_ascii_alphanumeric() || self.text[self.at] == b'.' || ((self.text[self.at] == b'-' || self.text[self.at] == b'+') && (self.text[self.at - 1] | 0x20) == b'e')) {
                    self.at += 1;
                }
                let word = std::str::from_utf8(&self.text[start..self.at]).unwrap_or("");
                match Decimal::parse(word) {
                    Some(d) => Ok(Value::Exact(d)),
                    None => Ok(Value::Approximate((self.eval)(word)? as f64)),
                }
            }
            Some(_) => {
                // A whole number or a label: up to the next operator.
                let start = self.at;
                while self.at < self.text.len() && !matches!(self.text[self.at], b'+' | b'-' | b'*' | b'/' | b'(' | b')') {
                    self.at += 1;
                }
                let word = std::str::from_utf8(&self.text[start..self.at]).unwrap_or("").trim();
                if word.is_empty() {
                    return Err(wrong().into());
                }
                let lower = word.to_ascii_lowercase();
                // vasm reads 0x as strtold does, as a whole number in hexadecimal.
                if let Some(hex) = lower.strip_prefix("0x") {
                    return u128::from_str_radix(hex, 16).map(|v| Value::Approximate(v as f64)).map_err(|_| wrong().into());
                }
                Ok(Value::Approximate((self.eval)(word)? as f64))
            }
            None => Err(wrong().into()),
        }
    }
}

/// Whether an expression starts with a decimal number (after any minus, plus or bracket), so that vasm works it
/// out in floating point.
pub fn starts_with_decimal(text: &str) -> bool {
    let rest = text.trim_start_matches(|c: char| c == '-' || c == '+' || c == '(' || c.is_whitespace());
    rest.starts_with(|c: char| c.is_ascii_digit())
}

/// A whole number written in hexadecimal too long for 64 bits (the bits of an extended number, #$3FFF0000...),
/// with a minus in front if it has one.
fn long_hex(text: &str) -> Option<u128> {
    let text = text.trim();
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, text),
    };
    let hex = rest.strip_prefix('$')?;
    if hex.is_empty() || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    // Only the lowest 96 bits are kept, as vasm keeps them.
    let hex = &hex[hex.len().saturating_sub(32)..];
    let v = u128::from_str_radix(hex, 16).ok()?;
    Some(if negative { v.wrapping_neg() } else { v })
}

/// A whole number's lowest `size` bytes (up to 12), as vasm keeps one: two's complement, so -1 is all ones.
pub fn whole(text: &str, size: usize, eval: &mut dyn FnMut(&str) -> Result<i64, String>) -> Result<Vec<u8>, String> {
    let v: u128 = match long_hex(text) {
        Some(v) => v,
        None => eval(text.trim())? as i128 as u128,
    };
    Ok(v.to_be_bytes()[16 - size.min(16)..].to_vec())
}

/// The bytes of a number in a format, from an expression: in floating point if it starts with a decimal number, the
/// bits themselves otherwise. `eval` works out a whole number's expression.
pub fn constant(text: &str, format: Format, eval: &mut dyn FnMut(&str) -> Result<i64, String>) -> Result<Vec<u8>, String> {
    let text = text.trim();
    let size = format.bytes();
    if !format.is_float() || !starts_with_decimal(text) {
        return whole(text, size, eval);
    }
    let mut parser = Parser { text: text.as_bytes(), at: 0, eval };
    let value = parser.sum()?;
    if parser.peek().is_some() {
        return Err(crate::i18n::tf!("{text} isn't a floating-point expression Limoniter can work out (it takes numbers, labels, + - * / and brackets)"));
    }
    let letter = format.letter().to_ascii_uppercase();
    let too_big = || crate::i18n::tf!("{text} is too big for .{letter}");
    if let Value::Approximate(v) = value
        && !v.is_finite()
    {
        return Err(too_big());
    }
    Ok(match (format, value) {
        (Format::Single, Value::Exact(d)) => single_bits(round_decimal(&d, SINGLE).ok_or_else(too_big)?).to_be_bytes().to_vec(),
        (Format::Single, Value::Approximate(v)) => {
            let s = v as f32;
            if !s.is_finite() {
                return Err(too_big());
            }
            s.to_bits().to_be_bytes().to_vec()
        }
        (Format::Double, Value::Exact(d)) => double_bits(round_decimal(&d, DOUBLE).ok_or_else(too_big)?).to_be_bytes().to_vec(),
        (Format::Double, Value::Approximate(v)) => v.to_bits().to_be_bytes().to_vec(),
        (Format::Extended, Value::Exact(d)) => extended_bytes(round_decimal(&d, EXTENDED).ok_or_else(too_big)?),
        (Format::Extended, Value::Approximate(v)) => extended_of_f64(v),
        (Format::Packed, Value::Exact(d)) => packed_bytes(&d).ok_or_else(|| crate::i18n::tf!("{text} is too big for .P: its exponent can only be -999 to 999"))?,
        (Format::Packed, Value::Approximate(v)) => packed_bytes(&Decimal::of_f64(v)).ok_or_else(|| crate::i18n::tf!("{text} is too big for .P: its exponent can only be -999 to 999"))?,
        _ => unreachable!(),
    })
}

// ---------- Back to the source ----------

/// How a float is written for the source: in decimal when the assembler and vasm would both make `bytes` from it,
/// and its bits in hexadecimal otherwise.
pub fn text(format: Format, bytes: &[u8]) -> String {
    decimal(format, bytes).unwrap_or_else(|| format!("${}", bytes.iter().map(|b| format!("{b:02X}")).collect::<String>()))
}

/// A number's shortest decimal (as Rust writes a double's or a single's), plainly when that's short and with an
/// exponent otherwise.
fn shortest(plain: String, exp: String, zero: bool, negative: bool) -> String {
    if zero {
        return if negative { "-0.0".into() } else { "0".into() };
    }
    if plain.len() <= exp.len() + 2 { plain } else { exp }
}

fn write_f64(v: f64) -> String {
    shortest(format!("{v}"), format!("{v:e}"), v == 0.0, v.is_sign_negative())
}

fn decimal(format: Format, bytes: &[u8]) -> Option<String> {
    match format {
        Format::Single => {
            let bits = u32::from_be_bytes(bytes.try_into().ok()?);
            let v = f32::from_bits(bits);
            let field = bits >> 23 & 0xFF;
            if field == 0xFF || (field == 0 && bits & 0x7F_FFFF != 0) {
                return None;
            }
            let text = shortest(format!("{v}"), format!("{v:e}"), v == 0.0, v.is_sign_negative());
            agrees(&text, format, bytes).then_some(text)
        }
        Format::Double => {
            let bits = u64::from_be_bytes(bytes.try_into().ok()?);
            let field = bits >> 52 & 0x7FF;
            if field == 0x7FF || (field == 0 && bits & 0xF_FFFF_FFFF_FFFF != 0) {
                return None;
            }
            let text = write_f64(f64::from_bits(bits));
            agrees(&text, format, bytes).then_some(text)
        }
        Format::Extended => {
            let se = u16::from_be_bytes([bytes[0], bytes[1]]);
            let mantissa = u64::from_be_bytes(bytes[4..12].try_into().ok()?);
            if bytes[2] != 0 || bytes[3] != 0 {
                return None;
            }
            let negative = se & 0x8000 != 0;
            let field = i32::from(se & 0x7FFF);
            let v = if mantissa == 0 && field == 0 {
                if negative { -0.0 } else { 0.0 }
            } else {
                // Only what a double holds exactly: vasm goes through one.
                let unbiased = field - 16383;
                if mantissa >> 63 == 0 || mantissa & 0x7FF != 0 || !(-1022..=1023).contains(&unbiased) {
                    return None;
                }
                let bits = (negative as u64) << 63 | ((unbiased + 1023) as u64) << 52 | (mantissa << 1 >> 12);
                f64::from_bits(bits)
            };
            let text = write_f64(v);
            agrees(&text, format, bytes).then_some(text)
        }
        Format::Packed => {
            let sign = if bytes[0] & 0x80 != 0 { "-" } else { "" };
            let nibbles: Vec<u8> = bytes.iter().flat_map(|b| [b >> 4, b & 15]).collect();
            // The exponent's three digits, and the seventeen of the number; all must be decimal digits, and the
            // bits that hold nothing (and the fourth exponent digit, which only the FPU writes) zero.
            if bytes[0] & 0x30 != 0 || nibbles[4] != 0 || nibbles[5] != 0 || nibbles[6] != 0 || nibbles[1..4].iter().chain(&nibbles[7..]).any(|n| *n > 9) {
                return None;
            }
            let exponent = i32::from(nibbles[1]) * 100 + i32::from(nibbles[2]) * 10 + i32::from(nibbles[3]);
            let exponent = if bytes[0] & 0x40 != 0 { -exponent } else { exponent };
            let digits = &nibbles[7..];
            let mut fraction: String = digits[1..].iter().map(|d| char::from(b'0' + d)).collect();
            while fraction.ends_with('0') {
                fraction.pop();
            }
            let mut text = format!("{sign}{}", digits[0]);
            if !fraction.is_empty() {
                text.push('.');
                text.push_str(&fraction);
            }
            if exponent != 0 {
                text.push_str(&format!("e{exponent}"));
            }
            if digits.iter().all(|d| *d == 0) && (exponent != 0 || bytes[0] & 0x40 != 0) {
                return None;
            }
            if text == "0" || text == "-0" {
                text.push_str(".0");
            }
            agrees(&text, format, bytes).then_some(text)
        }
        _ => None,
    }
}

/// Whether Limoniter's assembler and vasm both make `bytes` from `text`.
fn agrees(text: &str, format: Format, bytes: &[u8]) -> bool {
    let mut no_labels = |_: &str| -> Result<i64, String> { Err(String::new()) };
    if constant(text, format, &mut no_labels).ok().as_deref() != Some(bytes) {
        return false;
    }
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let Some(mut d) = Decimal::parse(unsigned) else { return false };
    d.negative = negative;
    // vasm reads the number in extended precision and rounds that again to the format.
    let Some(read) = round_decimal(&d, EXTENDED) else { return false };
    match format {
        Format::Single => round_again(read, EXTENDED, SINGLE).map(|b| single_bits(b).to_be_bytes().to_vec()).as_deref() == Some(bytes),
        Format::Double => round_again(read, EXTENDED, DOUBLE).map(|b| double_bits(b).to_be_bytes().to_vec()).as_deref() == Some(bytes),
        Format::Extended => round_again(read, EXTENDED, DOUBLE).map(|b| extended_of_f64(f64::from_bits(double_bits(b)))).as_deref() == Some(bytes),
        Format::Packed => {
            // vasm writes the double it reads with seventeen digits, and loses the sign of -0.
            let Some(b) = round_again(read, EXTENDED, DOUBLE) else { return false };
            let v = f64::from_bits(double_bits(b));
            if v == 0.0 {
                return !negative;
            }
            let again = format!("{:.16e}", v.abs());
            let Some(mut d2) = Decimal::parse(&again) else { return false };
            d2.negative = negative;
            packed_bytes(&d2).as_deref() == Some(bytes)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(text: &str, format: Format) -> String {
        // Labels and sums of numbers, as the assembler would work them out.
        let mut eval = |t: &str| -> Result<i64, String> {
            let mut sum = 0i64;
            for part in t.split('+') {
                let part = part.trim();
                sum += match part {
                    "three" => 3,
                    _ => match part.strip_prefix('$') {
                        Some(hex) => i64::from_str_radix(hex, 16).map_err(|e| e.to_string())?,
                        None => part.parse::<i64>().map_err(|e| e.to_string())?,
                    },
                };
            }
            Ok(sum)
        };
        constant(text, format, &mut eval).map(|b| b.iter().map(|b| format!("{b:02X}")).collect()).unwrap_or_else(|e| format!("error: {e}"))
    }

    /// The numbers vasm 2.0f made the same, and those it makes from a double where the format holds more.
    #[test]
    fn decimals_are_rounded_once_to_the_format() {
        for (text, format, want) in [
            ("1.5", Format::Single, "3FC00000"),
            ("1.5", Format::Double, "3FF8000000000000"),
            ("1.5", Format::Extended, "3FFF0000C000000000000000"),
            ("-2.5e-3", Format::Double, "BF647AE147AE147B"),
            ("0.1", Format::Single, "3DCCCCCD"),
            ("0.1", Format::Double, "3FB999999999999A"),
            ("3", Format::Single, "40400000"),
            ("10", Format::Extended, "40020000A000000000000000"),
            ("1e2", Format::Single, "42C80000"),
            ("1E+2", Format::Single, "42C80000"),
            ("10.", Format::Single, "41200000"),
            ("-0.0", Format::Single, "80000000"),
            ("-0", Format::Single, "80000000"),
            ("0", Format::Double, "0000000000000000"),
            ("4.9e-324", Format::Double, "0000000000000001"),
            ("1e-45", Format::Single, "00000001"),
            ("1e-400", Format::Double, "0000000000000000"),
            ("1/3", Format::Single, "3EAAAAAB"),
            ("3/2", Format::Single, "3FC00000"),
            ("1.5*2", Format::Single, "40400000"),
            ("-(1.5)", Format::Single, "BFC00000"),
            ("2+3", Format::Single, "40A00000"),
            ("0+$3", Format::Single, "40400000"),
            ("2*three", Format::Single, "40C00000"),
            // Past what a double holds: Limoniter keeps all 64 bits (vasm gives ...CCCCD000).
            ("0.1", Format::Extended, "3FFB0000CCCCCCCCCCCCCCCD"),
            ("-2.5e-3", Format::Extended, "BFF60000A3D70A3D70A3D70A"),
            ("1e-310", Format::Extended, "3BF9000093445B8731587EA3"),
            ("1e400", Format::Extended, "452F0000DA763FC8CB9FF9E6"),
            // Packed decimal, from the digits as written.
            ("3", Format::Packed, "000000030000000000000000"),
            ("1.5", Format::Packed, "000000015000000000000000"),
            ("0.1", Format::Packed, "400100010000000000000000"),
            ("123.456e100", Format::Packed, "010200012345600000000000"),
            ("-1.5e-300", Format::Packed, "C30000015000000000000000"),
            ("-0.0", Format::Packed, "800000000000000000000000"),
            ("1.23456789012345678", Format::Packed, "000000012345678901234568"),
            ("9.999999999999999999", Format::Packed, "000100010000000000000000"),
        ] {
            assert_eq!(make(text, format), want, "{text} as .{}", format.letter());
        }
    }

    /// What doesn't start with a decimal number is the bits themselves, as vasm takes it.
    #[test]
    fn other_expressions_are_the_bits() {
        for (text, format, want) in [
            ("$3fc00000", Format::Single, "3FC00000"),
            ("$3ff8000000000000", Format::Double, "3FF8000000000000"),
            ("$3fff0000c000000000000000", Format::Extended, "3FFF0000C000000000000000"),
            ("$1123456789abcdef012345678", Format::Extended, "123456789ABCDEF012345678"),
            ("$80000000", Format::Double, "0000000080000000"),
            ("-$100000000", Format::Double, "FFFFFFFF00000000"),
            ("-$1", Format::Extended, "FFFFFFFFFFFFFFFFFFFFFFFF"),
            ("-$3fc00000", Format::Single, "C0400000"),
            ("three", Format::Single, "00000003"),
            ("$3+0", Format::Single, "00000003"),
            ("$100000000", Format::Single, "00000000"),
            ("$80000000", Format::Packed, "000000000000000080000000"),
        ] {
            assert_eq!(make(text, format), want, "{text} as .{}", format.letter());
        }
        assert!(make("1e39", Format::Single).starts_with("error"));
        assert!(make("1e5000", Format::Extended).starts_with("error"));
        assert!(make("1e1000", Format::Packed).starts_with("error"));
    }

    /// Written back, a number is decimal only where both assemblers make the same bits of it, hexadecimal
    /// otherwise.
    #[test]
    fn floats_are_written_so_they_come_back_the_same() {
        for (format, hex, want) in [
            (Format::Single, "3FC00000", "1.5"),
            (Format::Single, "3DCCCCCD", "0.1"),
            (Format::Single, "80000000", "-0.0"),
            (Format::Single, "7FC00000", "$7FC00000"),
            (Format::Single, "7F800000", "$7F800000"),
            (Format::Single, "00000001", "$00000001"),
            (Format::Double, "3FB999999999999A", "0.1"),
            (Format::Double, "BF647AE147AE147B", "-0.0025"),
            (Format::Double, "0000000000000000", "0"),
            (Format::Extended, "3FFF0000C000000000000000", "1.5"),
            (Format::Extended, "3FFB0000CCCCCCCCCCCCD000", "$3FFB0000CCCCCCCCCCCCD000"),
            (Format::Extended, "3FFB0000CCCCCCCCCCCCCCCD", "$3FFB0000CCCCCCCCCCCCCCCD"),
            (Format::Extended, "7FFF00000000000000000000", "$7FFF00000000000000000000"),
            (Format::Extended, "40020000A000000000000000", "10"),
            (Format::Packed, "000000015000000000000000", "1.5"),
            (Format::Packed, "010200012345600000000000", "1.23456e102"),
            (Format::Packed, "400100010000000000000000", "$400100010000000000000000"),
            (Format::Packed, "800000000000000000000000", "$800000000000000000000000"),
            (Format::Packed, "000000000000000000000000", "0.0"),
            (Format::Packed, "7FFF00000000000000000000", "$7FFF00000000000000000000"),
        ] {
            let bytes: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
            let written = text(format, &bytes);
            assert_eq!(written, want, "{hex} as .{}", format.letter());
            assert_eq!(make(&written, format), hex, "{written} as .{}", format.letter());
        }
    }

    /// Every single-precision number with a few mantissas at every exponent, and a spread of doubles: what's
    /// written assembles back to the same bits.
    #[test]
    fn every_kind_of_single_and_double_comes_back() {
        for field in 0..256u32 {
            for fraction in [0u32, 1, 0x40_0000, 0x7F_FFFF, 0x12_3456, 0x55_5555] {
                let bits = field << 23 | fraction;
                for bits in [bits, bits | 0x8000_0000] {
                    let bytes = bits.to_be_bytes();
                    let written = text(Format::Single, &bytes);
                    assert_eq!(make(&written, Format::Single), format!("{bits:08X}"), "{written}");
                }
            }
        }
        let mut x = 0x1234_5678_9ABC_DEF0u64;
        for _ in 0..3000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let bytes = x.to_be_bytes();
            let written = text(Format::Double, &bytes);
            assert_eq!(make(&written, Format::Double), format!("{x:016X}"), "{written}");
            let mut extended = extended_of_f64(f64::from_bits(x));
            let written = text(Format::Extended, &extended);
            assert_eq!(make(&written, Format::Extended), extended.iter().map(|b| format!("{b:02X}")).collect::<String>(), "{written}");
            extended[11] ^= 1;
            assert!(text(Format::Extended, &extended).starts_with('$'));
        }
    }
}
