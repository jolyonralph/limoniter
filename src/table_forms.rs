//! More ways for the table generator (and the other tools that write tables) to write a table: BASIC DATA lines in
//! each dialect's own way, more assemblers' data lines, more languages' arrays, and Intel HEX and Motorola
//! S-record files.

use crate::table_gen::{Base, Size};

// ---------- BASIC ----------

/// A BASIC, and how it writes DATA lines: whether its lines are numbered, how it writes hex, and its longest line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dialect {
    /// Numbered, in decimal: what any BASIC reads.
    #[default]
    Any,
    Commodore,
    Spectrum,
    Atari,
    Applesoft,
    Bbc,
    /// BBC BASIC V and later (the Archimedes, BBC BASIC for Windows and SDL): no line numbers needed.
    BbcV,
    Msx,
    GwBasic,
    QBasic,
    Amos,
    Stos,
    Blitz,
}

impl Dialect {
    pub const ALL: [Dialect; 13] = [Dialect::Any, Dialect::Commodore, Dialect::Spectrum, Dialect::Atari, Dialect::Applesoft, Dialect::Bbc, Dialect::BbcV, Dialect::Msx, Dialect::GwBasic, Dialect::QBasic, Dialect::Amos, Dialect::Stos, Dialect::Blitz];

    pub fn name(self) -> &'static str {
        match self {
            Dialect::Any => "Any BASIC (numbered, decimal)",
            Dialect::Commodore => "Commodore BASIC (C64, VIC-20, PET)",
            Dialect::Spectrum => "Sinclair BASIC (ZX Spectrum)",
            Dialect::Atari => "Atari BASIC",
            Dialect::Applesoft => "Applesoft BASIC",
            Dialect::Bbc => "BBC BASIC (numbered)",
            Dialect::BbcV => "BBC BASIC V and later",
            Dialect::Msx => "MSX BASIC",
            Dialect::GwBasic => "GW-BASIC",
            Dialect::QBasic => "QBasic and QuickBASIC",
            Dialect::Amos => "AMOS",
            Dialect::Stos => "STOS",
            Dialect::Blitz => "Blitz Basic",
        }
    }

    pub fn numbered(self) -> bool {
        !matches!(self, Dialect::BbcV | Dialect::QBasic | Dialect::Amos | Dialect::Blitz)
    }

    /// How it writes 255 in hex; None if it has no hex numbers.
    fn hex_prefix(self) -> Option<&'static str> {
        match self {
            Dialect::Bbc | Dialect::BbcV => Some("&"),
            Dialect::Msx | Dialect::GwBasic | Dialect::QBasic => Some("&H"),
            Dialect::Amos | Dialect::Stos | Dialect::Blitz => Some("$"),
            _ => None,
        }
    }

    /// The longest line it takes, as typed or kept.
    fn longest_line(self) -> usize {
        match self {
            Dialect::Commodore => 80,
            Dialect::Atari => 120,
            Dialect::Applesoft | Dialect::Bbc => 238,
            _ => 250,
        }
    }

    fn comment(self) -> &'static str {
        if self == Dialect::Blitz { ";" } else { "REM" }
    }

    /// The array the table's read into: the label's first letter (Sinclair and Commodore BASIC only know the
    /// first letter or two), with BBC BASIC's % for whole numbers.
    fn array_name(self, label: &str, real: bool) -> String {
        let letter = label.chars().find(char::is_ascii_alphabetic).unwrap_or('T');
        let letter = if self == Dialect::Spectrum { letter.to_ascii_lowercase() } else { letter.to_ascii_uppercase() };
        if matches!(self, Dialect::Bbc | Dialect::BbcV) && !real { format!("{letter}%") } else { letter.to_string() }
    }
}

/// How BASIC DATA lines are written.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BasicOptions {
    pub dialect: Dialect,
    pub first: u32,
    pub step: u32,
    /// Real numbers with this many places, from the values divided by `scale` (2 to the fraction bits); None for
    /// whole numbers.
    pub places: Option<u32>,
    pub scale: f64,
    /// DIM and a READ loop before the DATA lines, to load it into an array.
    pub read_loop: bool,
}

impl Default for BasicOptions {
    fn default() -> Self {
        BasicOptions { dialect: Dialect::Any, first: 1000, step: 10, places: None, scale: 1.0, read_loop: false }
    }
}

fn basic_number(value: i64, size: Size, base: Base, options: &BasicOptions) -> String {
    if let Some(places) = options.places {
        let real = value as f64 / options.scale;
        let mut text = format!("{real:.*}", places as usize);
        // Most BASICs write .5 rather than 0.5, and it saves room.
        if text.starts_with("0.") {
            text.remove(0);
        } else if text.starts_with("-0.") {
            text.remove(1);
        }
        return text;
    }
    match (base, options.dialect.hex_prefix()) {
        (Base::Hex, Some(prefix)) => {
            let digits = size.bytes() * 2;
            let mask = if size == Size::Long { 0xFFFF_FFFF } else { (1i64 << (digits * 4)) - 1 };
            format!("{prefix}{:X}", value & mask)
        }
        _ => value.to_string(),
    }
}

/// DATA lines in a dialect's own way: numbered from `first` in `step`s or not, so many values to a line but never
/// longer than the dialect takes, with the label in a comment first and, if asked, a DIM and READ loop.
pub fn as_basic(values: &[i64], size: Size, base: Base, per_line: usize, label: &str, options: &BasicOptions) -> String {
    let dialect = options.dialect;
    let mut lines: Vec<String> = Vec::new();
    if !label.is_empty() {
        lines.push(format!("{} {label}", dialect.comment()));
    }
    if options.read_loop && !values.is_empty() {
        let name = dialect.array_name(label, options.places.is_some());
        let last = values.len() - 1;
        let index = if matches!(dialect, Dialect::Bbc | Dialect::BbcV) { "I%" } else if dialect == Dialect::Spectrum { "i" } else { "I" };
        // Sinclair BASIC's arrays count from 1.
        let (from, to, dim) = if dialect == Dialect::Spectrum { (1, values.len(), values.len()) } else { (0, last, last) };
        lines.push(format!("DIM {name}({dim})"));
        if dialect.numbered() {
            lines.push(format!("FOR {index}={from} TO {to}:READ {name}({index}):NEXT {index}"));
        } else {
            lines.push(format!("FOR {index}={from} TO {to}"));
            lines.push(format!("READ {name}({index})"));
            lines.push(format!("NEXT {}", if dialect == Dialect::Blitz { "" } else { index }).trim_end().to_string());
        }
    }
    // The values, as many to a line as asked, and as fit.
    let numbers: Vec<String> = values.iter().map(|v| basic_number(*v, size, base, options)).collect();
    let room = dialect.longest_line().saturating_sub(if dialect.numbered() { 6 } else { 0 } + 5);
    let mut current: Vec<&str> = Vec::new();
    let mut length = 0;
    for number in &numbers {
        let more = number.len() + usize::from(!current.is_empty());
        if !current.is_empty() && (current.len() >= per_line.max(1) || length + more > room) {
            lines.push(format!("DATA {}", current.join(",")));
            current.clear();
            length = 0;
        }
        length += number.len() + usize::from(!current.is_empty());
        current.push(number);
    }
    if !current.is_empty() {
        lines.push(format!("DATA {}", current.join(",")));
    }
    let mut out = String::new();
    let mut number = options.first;
    for line in lines {
        if dialect.numbered() {
            out.push_str(&format!("{number} {line}\n"));
            number += options.step.max(1);
        } else {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// The number to go on from in a BASIC listing: the last line's number and a step on; None if it has no numbers.
pub fn next_line_number<S: AsRef<str>>(lines: &[S], step: u32) -> Option<u32> {
    lines.iter().rev().find_map(|line| {
        let digits: String = line.as_ref().trim_start().chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u32>().ok()
    }).map(|last| last + step.max(1))
}

// ---------- Assemblers ----------

/// How an assembler writes hex.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HexStyle {
    /// $FF
    Dollar,
    /// 0xFF
    ZeroX,
    /// 0FFh
    HSuffix,
    /// &FF
    Ampersand,
}

/// A value in an assembler's hex, two's complement for its size, or in decimal.
pub fn asm_number(value: i64, size: Size, base: Base, style: HexStyle) -> String {
    if base == Base::Decimal {
        return value.to_string();
    }
    let digits = size.bytes() * 2;
    let mask = if size == Size::Long { 0xFFFF_FFFF } else { (1i64 << (digits * 4)) - 1 };
    let hex = format!("{:0digits$X}", value & mask);
    match style {
        HexStyle::Dollar => format!("${hex}"),
        HexStyle::ZeroX => format!("0x{hex}"),
        HexStyle::Ampersand => format!("&{hex}"),
        // A number can't start with a letter, so 0FFh rather than FFh.
        HexStyle::HSuffix => {
            if hex.starts_with(|c: char| c.is_ascii_alphabetic()) {
                format!("0{hex}h")
            } else {
                format!("{hex}h")
            }
        }
    }
}

/// A PIC table: a RETLW for each byte, under its label, for a computed GOTO to land on.
pub fn as_retlw(values: &[i64], base: Base, label: &str) -> String {
    let mut out = String::new();
    if !label.is_empty() {
        out.push_str(label);
        out.push('\n');
    }
    for value in values {
        out.push_str(&format!("        retlw   {}\n", asm_number(*value, Size::Byte, base, HexStyle::ZeroX)));
    }
    out
}

// ---------- Languages ----------

/// The languages whose arrays are written here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Pascal,
    Java,
    CSharp,
    Go,
    Swift,
    Kotlin,
    Lua,
    AmigaE,
    Fortran,
    Forth,
    Json,
    Csv,
    Arduino,
}

fn c_style(value: i64, size: Size, base: Base) -> String {
    match base {
        Base::Decimal => value.to_string(),
        Base::Hex => {
            let digits = size.bytes() * 2;
            if value < 0 { format!("-0x{:0digits$X}", -value) } else { format!("0x{value:0digits$X}") }
        }
    }
}

fn dollar_style(value: i64, size: Size, base: Base) -> String {
    match base {
        Base::Decimal => value.to_string(),
        Base::Hex => {
            let digits = size.bytes() * 2;
            if value < 0 { format!("-${:0digits$X}", -value) } else { format!("${value:0digits$X}") }
        }
    }
}

/// The values in lines of so many, each written by `number`, joined by `between`.
fn rows(values: &[i64], per_line: usize, indent: &str, between: &str, number: impl Fn(i64) -> String) -> Vec<String> {
    values.chunks(per_line.max(1)).map(|line| format!("{indent}{}", line.iter().map(|v| number(*v)).collect::<Vec<_>>().join(between))).collect()
}

/// A table as an array (or a list, or rows) in a language.
pub fn as_language(values: &[i64], size: Size, signed: bool, base: Base, per_line: usize, name: &str, language: Language) -> String {
    let name = if name.is_empty() { "table" } else { name };
    let n = values.len();
    let pick = |types: [&'static str; 6]| match (size, signed) {
        (Size::Byte, true) => types[0],
        (Size::Byte, false) => types[1],
        (Size::Word, true) => types[2],
        (Size::Word, false) => types[3],
        (Size::Long, true) => types[4],
        (Size::Long, false) => types[5],
    };
    let c = |v: i64| c_style(v, size, base);
    let dollar = |v: i64| dollar_style(v, size, base);
    let decimal = |v: i64| v.to_string();
    match language {
        Language::Pascal => {
            let kind = pick(["ShortInt", "Byte", "SmallInt", "Word", "LongInt", "LongWord"]);
            format!("const\n  {name}: array[0..{}] of {kind} = (\n{}\n  );\n", n.saturating_sub(1), rows(values, per_line, "    ", ", ", dollar).join(",\n"))
        }
        Language::Java => {
            // Java has no unsigned types, so an unsigned table goes in the next size up.
            let kind = pick(["byte", "short", "short", "int", "int", "long"]);
            let long = kind == "long";
            let number = |v: i64| if long { format!("{}L", c(v)) } else { c(v) };
            format!("static final {kind}[] {name} = {{\n{}\n}};\n", rows(values, per_line, "    ", ", ", number).join(",\n"))
        }
        Language::CSharp => {
            let kind = pick(["sbyte", "byte", "short", "ushort", "int", "uint"]);
            format!("static readonly {kind}[] {name} = {{\n{}\n}};\n", rows(values, per_line, "    ", ", ", c).join(",\n"))
        }
        Language::Go => {
            let kind = pick(["int8", "uint8", "int16", "uint16", "int32", "uint32"]);
            format!("var {name} = [{n}]{kind}{{\n{},\n}}\n", rows(values, per_line, "\t", ", ", c).join(",\n"))
        }
        Language::Swift => {
            let kind = pick(["Int8", "UInt8", "Int16", "UInt16", "Int32", "UInt32"]);
            format!("let {name}: [{kind}] = [\n{}\n]\n", rows(values, per_line, "    ", ", ", c).join(",\n"))
        }
        Language::Kotlin => {
            let long = size == Size::Long && !signed;
            let number = |v: i64| if long { format!("{}L", c(v)) } else { c(v) };
            format!("val {name} = {}(\n{}\n)\n", if long { "longArrayOf" } else { "intArrayOf" }, rows(values, per_line, "    ", ", ", number).join(",\n"))
        }
        Language::Lua => format!("local {name} = {{\n{}\n}}\n", rows(values, per_line, "  ", ", ", c).join(",\n")),
        Language::AmigaE => {
            // A typed list: CHAR, INT or LONG to an entry.
            let kind = match size {
                Size::Byte => "CHAR",
                Size::Word => "INT",
                Size::Long => "LONG",
            };
            format!("  {name}:=[\n{}\n  ]:{kind}\n", rows(values, per_line, "    ", ",", dollar).join(",\n"))
        }
        Language::Fortran => {
            let kind = size.bytes();
            let lines = rows(values, per_line, "      ", ", ", decimal);
            format!("integer(kind={kind}) :: {name}({n})\ndata {name} / &\n{} /\n", lines.join(", &\n"))
        }
        Language::Forth => {
            // A byte table is laid down with c, and anything larger a cell at a time.
            let comma = if size == Size::Byte { " c," } else { " ," };
            let number = |v: i64| format!("{}{comma}", dollar(v));
            format!("create {name}\n{}\n", rows(values, per_line, "  ", " ", number).join("\n"))
        }
        Language::Json => format!("[\n{}\n]\n", rows(values, per_line, "  ", ", ", decimal).join(",\n")),
        Language::Csv => rows(values, per_line, "", ",", decimal).join("\n") + "\n",
        Language::Arduino => {
            let kind = pick(["int8_t", "uint8_t", "int16_t", "uint16_t", "int32_t", "uint32_t"]);
            format!("#include <avr/pgmspace.h>\n\nconst {kind} {name}[{n}] PROGMEM = {{\n{}\n}};\n", rows(values, per_line, "    ", ", ", c).join(",\n"))
        }
    }
}

// ---------- Files ----------

/// Bytes as an Intel HEX file from an address: 16 to a record, with an extended linear address record each time
/// the address passes a 64K boundary, and the end-of-file record.
pub fn intel_hex(bytes: &[u8], address: u32) -> String {
    let record = |kind: u8, at: u16, data: &[u8]| {
        let mut sum = data.len() as u8;
        sum = sum.wrapping_add((at >> 8) as u8).wrapping_add(at as u8).wrapping_add(kind);
        let mut line = format!(":{:02X}{at:04X}{kind:02X}", data.len());
        for byte in data {
            line.push_str(&format!("{byte:02X}"));
            sum = sum.wrapping_add(*byte);
        }
        line.push_str(&format!("{:02X}\n", sum.wrapping_neg()));
        line
    };
    let mut out = String::new();
    let mut upper = None;
    let mut offset = 0usize;
    while offset < bytes.len() {
        let at = address.wrapping_add(offset as u32);
        if upper != Some(at >> 16) {
            upper = Some(at >> 16);
            if at >> 16 != 0 || offset > 0 {
                out.push_str(&record(4, 0, &((at >> 16) as u16).to_be_bytes()));
            }
        }
        // A record doesn't cross a 64K boundary.
        let to_boundary = 0x1_0000 - (at & 0xFFFF) as usize;
        let length = 16.min(bytes.len() - offset).min(to_boundary);
        out.push_str(&record(0, at as u16, &bytes[offset..offset + length]));
        offset += length;
    }
    out.push_str(":00000001FF\n");
    out
}

/// Bytes as Motorola S-records from an address: an S0 header with the name, S1 records (S2 or S3 for addresses
/// past 64K or 16M), and the matching S9, S8 or S7 end.
pub fn s_records(bytes: &[u8], address: u32, name: &str) -> String {
    let last = address as u64 + bytes.len() as u64;
    let (data_kind, end_kind, address_bytes) = if last <= 0x1_0000 {
        (1, 9, 2)
    } else if last <= 0x100_0000 {
        (2, 8, 3)
    } else {
        (3, 7, 4)
    };
    let record = |kind: u8, at: u32, width: usize, data: &[u8]| {
        let count = (width + data.len() + 1) as u8;
        let address = &at.to_be_bytes()[4 - width..];
        let mut sum = count;
        let mut line = format!("S{kind}{count:02X}");
        for byte in address.iter().chain(data) {
            line.push_str(&format!("{byte:02X}"));
            sum = sum.wrapping_add(*byte);
        }
        line.push_str(&format!("{:02X}\n", !sum));
        line
    };
    let mut out = record(0, 0, 2, &name.bytes().take(40).collect::<Vec<_>>());
    for (index, chunk) in bytes.chunks(16).enumerate() {
        out.push_str(&record(data_kind, address.wrapping_add(index as u32 * 16), address_bytes, chunk));
    }
    out.push_str(&record(end_kind, address, address_bytes, &[]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_data_is_written_in_each_dialects_way() {
        let values = [0, 127, 255, -1];
        let bbc = BasicOptions { dialect: Dialect::Bbc, first: 100, step: 5, ..BasicOptions::default() };
        assert_eq!(as_basic(&values, Size::Byte, Base::Hex, 8, "wave", &bbc), "100 REM wave\n105 DATA &0,&7F,&FF,&FF\n");
        let qbasic = BasicOptions { dialect: Dialect::QBasic, read_loop: true, ..BasicOptions::default() };
        assert_eq!(as_basic(&values[..2], Size::Word, Base::Hex, 8, "wave", &qbasic), "REM wave\nDIM W(1)\nFOR I=0 TO 1\nREAD W(I)\nNEXT I\nDATA &H0,&H7F\n");
        // No hex in Commodore BASIC, and its lines kept to 80 characters.
        let c64 = BasicOptions { dialect: Dialect::Commodore, ..BasicOptions::default() };
        let long = as_basic(&[1000; 40], Size::Word, Base::Hex, 64, "", &c64);
        assert!(long.lines().all(|l| l.len() <= 80) && long.lines().count() > 1, "{long}");
        assert!(long.starts_with("1000 DATA 1000,1000"));
        // Sinclair BASIC's arrays count from 1.
        let spectrum = BasicOptions { dialect: Dialect::Spectrum, read_loop: true, ..BasicOptions::default() };
        assert!(as_basic(&values, Size::Byte, Base::Decimal, 8, "sine", &spectrum).contains("1010 DIM s(4)\n1020 FOR i=1 TO 4:READ s(i):NEXT i\n"));
        // Real numbers, from fixed point.
        let real = BasicOptions { places: Some(3), scale: 256.0, dialect: Dialect::Blitz, ..BasicOptions::default() };
        assert_eq!(as_basic(&[128, -64, 512], Size::Word, Base::Hex, 8, "", &real), "DATA .500,-.250,2.000\n");
        assert_eq!(next_line_number(&["10 PRINT", "20 GOTO 10", ""], 10), Some(30));
        assert_eq!(next_line_number(&["PRINT"], 10), None);
    }

    #[test]
    fn assemblers_hex_is_written_their_way() {
        assert_eq!(asm_number(255, Size::Byte, Base::Hex, HexStyle::HSuffix), "0FFh");
        assert_eq!(asm_number(0x12, Size::Byte, Base::Hex, HexStyle::HSuffix), "12h");
        assert_eq!(asm_number(-1, Size::Word, Base::Hex, HexStyle::Ampersand), "&FFFF");
        assert_eq!(as_retlw(&[1, 255], Base::Hex, "table"), "table\n        retlw   0x01\n        retlw   0xFF\n");
    }

    #[test]
    fn languages_have_their_own_arrays() {
        let values = [1, -2, 3];
        assert_eq!(as_language(&values, Size::Byte, true, Base::Decimal, 8, "t", Language::Pascal), "const\n  t: array[0..2] of ShortInt = (\n    1, -2, 3\n  );\n");
        assert_eq!(as_language(&values, Size::Word, true, Base::Decimal, 8, "t", Language::Go), "var t = [3]int16{\n\t1, -2, 3,\n}\n");
        assert_eq!(as_language(&[1, 2], Size::Byte, false, Base::Hex, 8, "t", Language::Forth), "create t\n  $01 c, $02 c,\n");
        assert_eq!(as_language(&[1, 2], Size::Word, false, Base::Hex, 8, "t", Language::AmigaE), "  t:=[\n    $0001,$0002\n  ]:INT\n");
        assert_eq!(as_language(&[1, 2, 3], Size::Byte, false, Base::Hex, 2, "", Language::Csv), "1,2\n3\n");
        assert!(as_language(&[1], Size::Long, false, Base::Hex, 8, "t", Language::Java).contains("long[] t = {\n    0x00000001L\n};"));
        assert!(as_language(&[1], Size::Byte, false, Base::Hex, 8, "t", Language::Arduino).contains("const uint8_t t[1] PROGMEM = {"));
        assert_eq!(as_language(&[1, 2], Size::Word, true, Base::Decimal, 1, "t", Language::Fortran), "integer(kind=2) :: t(2)\ndata t / &\n      1, &\n      2 /\n");
    }

    #[test]
    fn hex_files_are_checked_as_they_should_be() {
        // The example every Intel HEX description gives.
        let hex = intel_hex(&[0x02, 0x33, 0x7A], 0x0030);
        assert_eq!(hex, ":0300300002337A1E\n:00000001FF\n");
        // Past 64K, an extended address record comes first.
        assert!(intel_hex(&[1], 0x1_0000).starts_with(":020000040001F9\n"));
        let s = s_records(&[0x01, 0x02], 0x1000, "T");
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines, ["S004000054A7", "S10510000102E7", "S9031000EC"]);
    }
}
