//! The value inspector: the bytes at the hex cursor read as numbers in each of the ways a machine might have
//! written them. Today's integers and IEEE floats in both byte orders, and the older formats that files from
//! mainframes and minicomputers hold:
//!
//! - IBM hexadecimal floating point (System/360 and after, and the ICL 2900, which uses the same form): a sign, a
//!   7-bit exponent of 16 biased by 64, and a 24-bit or 56-bit fraction, big-endian (IBM's *Principles of Operation*).
//! - DEC VAX F, D, G and H floating point: a sign, an exponent biased by 128 (F, D), 1024 (G) or 16384 (H), and a fraction with a
//!   hidden leading 1, value 0.1f × 2^(e − bias); stored as 16-bit little-endian words, most significant word first
//!   (*VAX Architecture Reference Manual*, chapter 1). The PDP-11's FP11 floats are F and D.
//! - Packed decimal (IBM's, and COBOL's COMP-3): two digits a byte, the sign in the last half-byte, C, A, E or F for
//!   plus and D or B for minus.
//! - RADIX-50: three characters in each 16-bit little-endian word, as PDP-11 and VAX file names and symbols hold them
//!   (space, A to Z, $, ., an unused code, 0 to 9; value = c1 × 1600 + c2 × 40 + c3).

/// One reading of the bytes: what it is, and what the bytes say as that.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub label: &'static str,
    pub value: String,
}

/// The most bytes any reading looks at.
pub const MOST_BYTES: usize = 16;

/// Every reading the bytes at the cursor allow. `bytes` starts at the cursor and has up to `MOST_BYTES`.
pub fn readings(bytes: &[u8]) -> Vec<Reading> {
    let mut out = Vec::new();
    let mut add = |label: &'static str, value: Option<String>| {
        if let Some(value) = value {
            out.push(Reading { label, value });
        }
    };
    let take = |n: usize| bytes.get(..n);
    add("Binary", take(1).map(|b| format!("{:08b}", b[0])));
    add("8-bit", take(1).map(|b| if b[0] < 128 { b[0].to_string() } else { format!("{} or {}", b[0], b[0] as i8) }));
    let le = |n: usize| take(n).map(|b| b.iter().rev().fold(0u64, |v, x| v << 8 | u64::from(*x)));
    let be = |n: usize| take(n).map(|b| b.iter().fold(0u64, |v, x| v << 8 | u64::from(*x)));
    for (label, value, bits) in [("16-bit, little-endian", le(2), 16), ("16-bit, big-endian", be(2), 16), ("32-bit, little-endian", le(4), 32), ("32-bit, big-endian", be(4), 32), ("64-bit, little-endian", le(8), 64), ("64-bit, big-endian", be(8), 64)] {
        add(label, value.map(|v| signed_and_unsigned(v, bits)));
    }
    add("Float, little-endian", take(4).map(|b| shown_f64(f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])))));
    add("Float, big-endian", take(4).map(|b| shown_f64(f64::from(f32::from_be_bytes([b[0], b[1], b[2], b[3]])))));
    add("Double, little-endian", take(8).map(|b| shown_f64(f64::from_le_bytes(b.try_into().unwrap()))));
    add("Double, big-endian", take(8).map(|b| shown_f64(f64::from_be_bytes(b.try_into().unwrap()))));
    add("IBM hex float, 32-bit", take(4).map(|b| shown_f64(ibm_float(b))));
    add("IBM hex float, 64-bit", take(8).map(|b| shown_f64(ibm_float(b))));
    add("VAX F float", take(4).and_then(|b| vax_float(b, VaxKind::F)).map(shown_vax));
    add("VAX D float", take(8).and_then(|b| vax_float(b, VaxKind::D)).map(shown_vax));
    add("VAX G float", take(8).and_then(|b| vax_float(b, VaxKind::G)).map(shown_vax));
    add("VAX H float", take(16).and_then(|b| vax_float(b, VaxKind::H)).map(shown_vax));
    add("Packed decimal", packed_decimal(bytes));
    add("RADIX-50", take(2).map(|_| radix50(bytes)));
    out
}

fn signed_and_unsigned(value: u64, bits: u32) -> String {
    let signed = if bits == 64 { value as i64 } else { ((value << (64 - bits)) as i64) >> (64 - bits) };
    if signed < 0 { format!("{value} or {signed}") } else { value.to_string() }
}

/// A float as people write it: exact for whole numbers, enough digits to come back the same otherwise.
fn shown_f64(value: f64) -> String {
    if value.is_nan() {
        return crate::i18n::tr("Not a number").into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity".into() } else { "\u{2212}Infinity".into() };
    }
    let plain = format!("{value}");
    if plain.len() <= 24 { plain } else { format!("{value:e}") }
}

/// IBM hexadecimal floating point, 4 or 8 bytes, big-endian.
pub fn ibm_float(bytes: &[u8]) -> f64 {
    let sign = if bytes[0] & 0x80 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from(bytes[0] & 0x7F) - 64;
    let fraction = bytes[1..].iter().fold(0u64, |v, b| v << 8 | u64::from(*b));
    let fraction_bits = 8 * (bytes.len() as i32 - 1);
    sign * fraction as f64 * 2f64.powi(4 * exponent - fraction_bits)
}

#[derive(Clone, Copy)]
enum VaxKind {
    F,
    D,
    G,
    H,
}

/// What a VAX float holds: a number, or the reserved operand (sign set, exponent 0), which faults on a VAX.
#[derive(Debug, PartialEq)]
enum Vax {
    Number(f64),
    Reserved,
}

fn shown_vax(value: Vax) -> String {
    match value {
        Vax::Number(n) => shown_f64(n),
        Vax::Reserved => crate::i18n::tr("Reserved operand").into(),
    }
}

/// A VAX float, from words stored little-endian with the most significant word first.
fn vax_float(bytes: &[u8], kind: VaxKind) -> Option<Vax> {
    // The words in order of significance, put together into one big-endian number.
    let bits = bytes.chunks(2).fold(0u128, |v, w| v << 16 | u128::from(u16::from_le_bytes([w[0], w[1]])));
    let total = 8 * bytes.len() as u32;
    let (exponent_bits, bias) = match kind {
        VaxKind::F | VaxKind::D => (8, 128),
        VaxKind::G => (11, 1024),
        VaxKind::H => (15, 16384),
    };
    let fraction_bits = total - 1 - exponent_bits;
    let sign = bits >> (total - 1) & 1;
    let exponent = (bits >> fraction_bits) & ((1 << exponent_bits) - 1);
    let fraction = bits & ((1u128 << fraction_bits) - 1);
    if exponent == 0 {
        return Some(if sign == 1 { Vax::Reserved } else { Vax::Number(0.0) });
    }
    // 0.1f: the hidden bit is worth a half. A double holds the top 53 bits of an H float's 113.
    let mantissa = (fraction | 1 << fraction_bits) as f64 / 2f64.powi(fraction_bits as i32 + 1);
    let value = mantissa * 2f64.powi(exponent as i32 - bias);
    Some(Vax::Number(if sign == 1 { -value } else { value }))
}

/// Packed decimal up to the first byte whose low half is a sign, if every half-byte before it is a digit.
pub fn packed_decimal(bytes: &[u8]) -> Option<String> {
    let mut digits = String::new();
    for byte in bytes {
        let (high, low) = (byte >> 4, byte & 0xF);
        if high > 9 {
            return None;
        }
        digits.push(char::from(b'0' + high));
        match low {
            0..=9 => digits.push(char::from(b'0' + low)),
            0xA | 0xC | 0xE | 0xF => return Some(trim_zeros(&digits).to_string()),
            _ => return Some(format!("\u{2212}{}", trim_zeros(&digits))),
        }
    }
    None
}

fn trim_zeros(digits: &str) -> &str {
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() { "0" } else { trimmed }
}

const RADIX50: &[u8; 40] = b" ABCDEFGHIJKLMNOPQRSTUVWXYZ$.%0123456789";

/// The RADIX-50 characters in up to the first four words, three to a word; a word too big to be RADIX-50 shows
/// as a question mark.
pub fn radix50(bytes: &[u8]) -> String {
    let mut out = String::new();
    for word in bytes.as_chunks::<2>().0.iter().take(4) {
        let value = u16::from_le_bytes(*word);
        if value >= 40 * 40 * 40 {
            out += "???";
            continue;
        }
        for divisor in [1600, 40, 1] {
            out.push(char::from(RADIX50[usize::from(value / divisor % 40)]));
        }
    }
    format!("\u{201C}{out}\u{201D}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(bytes: &[u8], label: &str) -> String {
        readings(bytes).into_iter().find(|r| r.label == label).map(|r| r.value).unwrap_or_default()
    }

    #[test]
    fn integers_both_ways_round_with_their_signed_reading() {
        let bytes = [0xFE, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0];
        assert_eq!(reading(&bytes, "8-bit"), "254 or -2");
        assert_eq!(reading(&bytes, "16-bit, little-endian"), "65534 or -2");
        assert_eq!(reading(&bytes, "16-bit, big-endian"), "65279 or -257");
        assert_eq!(reading(&bytes, "32-bit, little-endian"), "4294967294 or -2");
        assert_eq!(reading(&bytes, "64-bit, little-endian"), "4294967294");
        assert_eq!(reading(&[1], "Binary"), "00000001");
        assert!(reading(&[1], "16-bit, little-endian").is_empty(), "too few bytes, no reading");
    }

    #[test]
    fn ieee_floats() {
        assert_eq!(reading(&1.5f32.to_le_bytes(), "Float, little-endian"), "1.5");
        assert_eq!(reading(&(-0.25f64).to_be_bytes(), "Double, big-endian"), "-0.25");
        assert_eq!(reading(&f32::NAN.to_le_bytes(), "Float, little-endian"), "Not a number");
    }

    #[test]
    fn ibm_hexadecimal_floating_point() {
        // Principles of Operation's examples: 1.0 is 41 10 00 00, -118.625 is C2 76 A0 00.
        assert_eq!(ibm_float(&[0x41, 0x10, 0, 0]), 1.0);
        assert_eq!(ibm_float(&[0xC2, 0x76, 0xA0, 0]), -118.625);
        assert_eq!(ibm_float(&[0x40, 0x80, 0, 0, 0, 0, 0, 0]), 0.5);
        assert_eq!(ibm_float(&[0, 0, 0, 0]), 0.0);
    }

    #[test]
    fn vax_floats_with_their_words_swapped() {
        // 1.0 in F is 0x40800000 as a longword of words high first: stored 80 40 00 00.
        assert_eq!(vax_float(&[0x80, 0x40, 0, 0], VaxKind::F), Some(Vax::Number(1.0)));
        assert_eq!(vax_float(&[0x80, 0xC0, 0, 0], VaxKind::F), Some(Vax::Number(-1.0)));
        // 0.5 is 0x40000000, and in D the same with four more zero bytes.
        assert_eq!(vax_float(&[0, 0x40, 0, 0, 0, 0, 0, 0], VaxKind::D), Some(Vax::Number(0.5)));
        // G's exponent is 11 bits biased by 1024: 1.0 is 0x4010 0000 0000 0000.
        assert_eq!(vax_float(&[0x10, 0x40, 0, 0, 0, 0, 0, 0], VaxKind::G), Some(Vax::Number(1.0)));
        // 3.0 in F: 0.11 binary × 2^2, exponent 130, fraction 0x400000.
        assert_eq!(vax_float(&[0x40, 0x41, 0, 0], VaxKind::F), Some(Vax::Number(3.0)));
        // H's exponent is 15 bits biased by 16384: 1.0 is 0x4001 then zeros.
        let mut h = [0u8; 16];
        h[..2].copy_from_slice(&[0x01, 0x40]);
        assert_eq!(vax_float(&h, VaxKind::H), Some(Vax::Number(1.0)));
        assert_eq!(vax_float(&[0, 0x80, 0, 0], VaxKind::F), Some(Vax::Reserved));
        assert_eq!(vax_float(&[0, 0, 0, 0], VaxKind::F), Some(Vax::Number(0.0)));
    }

    #[test]
    fn packed_decimal_and_its_signs() {
        assert_eq!(packed_decimal(&[0x12, 0x34, 0x5C]).as_deref(), Some("12345"));
        assert_eq!(packed_decimal(&[0x00, 0x7D]).as_deref(), Some("\u{2212}7"));
        assert_eq!(packed_decimal(&[0x0F]).as_deref(), Some("0"));
        assert_eq!(packed_decimal(&[0x1A, 0x2C]).as_deref(), Some("1"), "A is a plus sign too");
        assert_eq!(packed_decimal(&[0xA1, 0x2C]), None);
        assert_eq!(packed_decimal(&[0x12, 0x34]), None, "no sign within the bytes");
    }

    #[test]
    fn radix_50_three_characters_a_word() {
        // "ABC" = 1×1600 + 2×40 + 3 = 1683 = 0x0693; "DEF" = 4×1600 + 5×40 + 6 = 6606 = 0x19CE.
        assert_eq!(radix50(&[0x93, 0x06, 0xCE, 0x19]), "\u{201C}ABCDEF\u{201D}");
        assert_eq!(radix50(&[0xFF, 0xFF]), "\u{201C}???\u{201D}");
    }
}
