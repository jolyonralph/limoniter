//! Small text tools on the Coding tab. Nothing here draws anything.

use sha2::{Digest, Sha256};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

/// How a language writes comments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommentMarks {
    pub line: Option<&'static str>,
    pub block: Option<(&'static str, &'static str)>,
}

/// The comment marks for a language, by the name the highlighter gives it. None if there's no entry for it.
pub fn comment_marks(language: &str) -> Option<CommentMarks> {
    const C_BLOCK: Option<(&str, &str)> = Some(("/*", "*/"));
    const HTML: Option<(&str, &str)> = Some(("<!--", "-->"));
    let line = |mark: &'static str| Some(CommentMarks { line: Some(mark), block: None });
    let c_style = Some(CommentMarks { line: Some("//"), block: C_BLOCK });
    match language {
        "C" | "C++" | "C#" | "Java" | "JavaScript" | "JavaScript (Babel)" | "TypeScript" | "TypeScriptReact" | "Go" | "Rust" | "Swift" | "Kotlin" | "Scala" | "Dart" | "D" | "Objective-C" | "Objective-C++" | "ActionScript" | "Groovy" | "Zig" | "PHP" | "SCSS" | "Sass" | "LESS" | "Solidity" | "Verilog" => c_style,
        "CSS" => Some(CommentMarks { line: None, block: C_BLOCK }),
        "HTML" | "XML" | "Markdown" | "MultiMarkdown" | "SVG" | "HTML (Rails)" => Some(CommentMarks { line: None, block: HTML }),
        "Python" | "Ruby" | "Perl" | "R" | "Bourne Again Shell (bash)" | "Shell-Unix-Generic" | "Fish" | "YAML" | "TOML" | "Makefile" | "Dockerfile" | "CMake" | "PowerShell" | "Nim" | "Elixir" | "Julia" | "Git Config" | "NGINX" | "Puppet" => line("#"),
        "SQL" | "Lua" | "Haskell" | "Ada" | "Elm" | "VHDL" => line("--"),
        "Batch File" => line("REM"),
        "VimL" => line("\""),
        "Visual Basic" | "VBScript" | "ASP" => line("'"),
        "Lisp" | "Clojure" | "Scheme" | "Assembly x86_64" | "Assembly 80x86" | "Assembly 680x0" | "Assembly 6502" | "Assembly Z80" | "INI" | "Racket" => line(";"),
        "LaTeX" | "TeX" | "MATLAB" | "Erlang" | "BibTeX" => line("%"),
        _ => None,
    }
}

/// Reads a file to put its text in at the cursor. It's read in its own encoding, whatever the document's is.
pub fn text_of_file(path: &std::path::Path) -> Result<String, String> {
    const MOST: u64 = 64 * 1024 * 1024;
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MOST {
        return Err("That file is too large to insert. It can be up to 64 MB.".to_string());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if crate::codec::looks_binary(&bytes) {
        return Err("That looks like a binary file rather than text, so it hasn't been inserted.".to_string());
    }
    let found = crate::codec::detect(&bytes);
    let mut format = crate::codec::TextFormat { charset: found.charset, ..crate::codec::TextFormat::default() };
    let decoded = crate::codec::decode(&bytes, &mut format);
    Ok(decoded.lines.iter().map(|line| format.readable_text(line)).collect::<Vec<_>>().join("\n"))
}

/// The ways of writing a colour that Insert Color offers.
pub const COLOR_FORMS: [&str; 3] = ["#RRGGBB", "rgb(r, g, b)", "r, g, b"];

pub fn color_text(rgb: [u8; 3], form: usize) -> String {
    let [r, g, b] = rgb;
    match form {
        1 => format!("rgb({r}, {g}, {b})"),
        2 => format!("{r}, {g}, {b}"),
        _ => format!("#{r:02X}{g:02X}{b:02X}"),
    }
}

/// Lays JSON out with indents, or squeezes it onto one line.
pub fn format_json(text: &str, pretty: bool) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| json_error_text(&e))?;
    let formatted = if pretty { serde_json::to_string_pretty(&value) } else { serde_json::to_string(&value) };
    formatted.map_err(|e| e.to_string())
}

/// Words a JSON problem for the person.
pub fn json_error_text(error: &serde_json::Error) -> String {
    let message = error.to_string();
    let reason = if message.contains("recursion limit") {
        "max level of depth reached"
    } else if message.contains("EOF while parsing a string") {
        "no closing quotation mark"
    } else if message.contains("EOF while parsing") {
        "no closing parenthesis / bracket found"
    } else if message.contains("expected `:`") {
        "missing colon"
    } else if message.contains("expected `,`") {
        "missing comma"
    } else if message.contains("number") {
        "wrong number syntax"
    } else if message.contains("unicode") {
        "wrong unicode code"
    } else if message.contains("escape") {
        "wrong string specifier"
    } else if message.contains("expected value") || message.contains("expected ident") {
        "wrong value"
    } else {
        "unknown error"
    };
    format!("JSON error ({}, {}): {reason}", error.line(), error.column())
}

pub fn sha256_text(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A new random GUID, in upper case as Windows writes them.
pub fn new_guid() -> String {
    let mut bytes = [0u8; 16];
    for chunk in bytes.chunks_mut(8) {
        let value = RandomState::new().build_hasher().finish().to_le_bytes();
        chunk.copy_from_slice(&value[..chunk.len()]);
    }
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

/// Reads a number written as decimal, 0x hex, 0b binary or 0o octal, and shows it in each.
pub fn convert_number(text: &str) -> Option<String> {
    let t = text.trim().replace('_', "");
    let lower = t.to_lowercase();
    let value = if let Some(hex) = lower.strip_prefix("0x") {
        u128::from_str_radix(hex, 16).ok()?
    } else if let Some(binary) = lower.strip_prefix("0b") {
        u128::from_str_radix(binary, 2).ok()?
    } else if let Some(octal) = lower.strip_prefix("0o") {
        u128::from_str_radix(octal, 8).ok()?
    } else {
        lower.parse::<u128>().ok()?
    };
    Some(format!("Decimal: {value}\nHex: 0x{value:X}\nOctal: 0o{value:o}\nBinary: 0b{value:b}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_have_their_own_comment_marks_and_unknown_ones_have_none() {
        assert_eq!(comment_marks("Rust").unwrap().line, Some("//"));
        for assembler in ["Assembly x86_64", "Assembly 80x86", "Assembly 680x0", "Assembly 6502", "Assembly Z80"] {
            assert_eq!(comment_marks(assembler).unwrap().line, Some(";"), "{assembler}");
        }
        assert_eq!(comment_marks("Python").unwrap().line, Some("#"));
        assert_eq!(comment_marks("SQL").unwrap().line, Some("--"));
        assert_eq!(comment_marks("HTML").unwrap().block, Some(("<!--", "-->")));
        assert_eq!(comment_marks("CSS").unwrap().line, None);
        assert_eq!(comment_marks("JSON"), None, "JSON has no comments");
        assert_eq!(comment_marks("No highlighting"), None);
    }

    #[test]
    fn every_comment_entry_can_comment_something() {
        for name in ["C", "Rust", "CSS", "HTML", "Python", "SQL", "Batch File", "VimL", "ASP", "Lisp", "LaTeX"] {
            let marks = comment_marks(name).unwrap();
            assert!(marks.line.is_some() || marks.block.is_some(), "{name}");
        }
    }

    #[test]
    fn colours_are_written_each_way() {
        assert_eq!(color_text([255, 16, 0], 0), "#FF1000");
        assert_eq!(color_text([255, 16, 0], 1), "rgb(255, 16, 0)");
        assert_eq!(color_text([255, 16, 0], 2), "255, 16, 0");
    }

    #[test]
    fn a_file_inserted_is_read_in_its_own_encoding_and_binary_is_refused() {
        let dir = std::env::temp_dir().join(format!("limoniter-insert-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let text = dir.join("t.txt");
        std::fs::write(&text, b"\xEF\xBB\xBFcaf\xC3\xA9\r\nline two\n").unwrap();
        assert_eq!(text_of_file(&text).unwrap(), "caf\u{e9}\nline two\n");
        let bin = dir.join("b.bin");
        std::fs::write(&bin, b"MZ\x00\x01\x02").unwrap();
        assert!(text_of_file(&bin).unwrap_err().contains("binary"));
        assert!(text_of_file(&dir.join("missing")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn json_formatting_keeps_the_order_of_keys() {
        assert_eq!(format_json(r#"{"b":1,"a":[true]}"#, true).unwrap(), "{\n  \"b\": 1,\n  \"a\": [\n    true\n  ]\n}");
        assert_eq!(format_json("{ \"b\" : 1 ,\n \"a\" : 2 }", false).unwrap(), r#"{"b":1,"a":2}"#);
        assert_eq!(format_json("{\n  \"a\" 1}", true).unwrap_err(), "JSON error (2, 7): missing colon");
        assert_eq!(format_json("[1, 2", true).unwrap_err(), "JSON error (1, 5): no closing parenthesis / bracket found");
        assert_eq!(format_json("\"abc", true).unwrap_err(), "JSON error (1, 4): no closing quotation mark");
    }

    #[test]
    fn hashes_are_correct() {
        assert_eq!(sha256_text("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn guids_have_the_right_shape_and_differ() {
        let (a, b) = (new_guid(), new_guid());
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        let parts: Vec<&str> = a.split('-').collect();
        assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), [8, 4, 4, 4, 12]);
        assert!(parts[2].starts_with('4'), "version 4: {a}");
        assert!("89AB".contains(&parts[3][..1]), "the standard variant: {a}");
        assert!(a.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
    }

    #[test]
    fn numbers_convert_from_any_base() {
        let answer = "Decimal: 255\nHex: 0xFF\nOctal: 0o377\nBinary: 0b11111111";
        for text in ["255", " 0xff ", "0XFF", "0b1111_1111", "0o377"] {
            assert_eq!(convert_number(text).as_deref(), Some(answer), "for {text}");
        }
        assert_eq!(convert_number("hello"), None);
        assert_eq!(convert_number("0xZZ"), None);
    }
}
