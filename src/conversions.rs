//! Format, Convert: Base64 to and from text, and JSON to XML and back.
//!
//! JSON becomes XML the way most converters do it: an object's members become elements named after them, an array
//! becomes the same element repeated, a member whose name starts with `@` becomes an attribute and one named `#text`
//! the element's text. XML comes back the same way, with text that reads as a number, `true`, `false` or nothing
//! becoming that in JSON. An array of one comes back as a single value, since XML can't tell the two apart.

use serde_json::{Map, Value};

/// One of the conversions in Format, Convert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conversion {
    Base64Encode,
    Base64Decode,
    JsonToXml,
    XmlToJson,
}

impl Conversion {
    pub fn title(self) -> &'static str {
        match self {
            Conversion::Base64Encode => "Base64 encode",
            Conversion::Base64Decode => "Base64 decode",
            Conversion::JsonToXml => "JSON to XML",
            Conversion::XmlToJson => "XML to JSON",
        }
    }

    /// The text converted, or what's wrong with it.
    pub fn apply(self, text: &str) -> Result<String, String> {
        match self {
            Conversion::Base64Encode => Ok(base64_encode(text.as_bytes(), true)),
            Conversion::Base64Decode => base64_decode_text(text),
            Conversion::JsonToXml => json_to_xml(text),
            Conversion::XmlToJson => xml_to_json(text),
        }
    }
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// The bytes as Base64 (RFC 4648), with `=` padding, in lines of 76 characters when `wrap` is set, as MIME has them.
pub fn base64_encode(bytes: &[u8], wrap: bool) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4 + bytes.len() / 57 * 2);
    for (index, chunk) in bytes.chunks(3).enumerate() {
        if wrap && index > 0 && index % 19 == 0 {
            out.push('\n');
        }
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(BASE64[(n >> (18 - 6 * i) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Base64 back to its bytes. Spaces and line ends are passed over, the URL-safe letters `-` and `_` are taken for
/// `+` and `/`, and the padding can be left off.
pub fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut bits = 0u32;
    let mut count = 0;
    let mut padding = 0;
    for (index, c) in text.chars().enumerate() {
        let value = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' | '-' => 62,
            '/' | '_' => 63,
            '=' => {
                padding += 1;
                continue;
            }
            c if c.is_whitespace() => continue,
            other => return Err(format!("\u{201C}{other}\u{201D} (character {}) isn't used in Base64.", index + 1)),
        };
        if padding > 0 {
            return Err("There's more after the = at the end.".into());
        }
        bits = bits << 6 | value;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    if count == 6 {
        return Err("The Base64 is one letter too long or short: it can't end there.".into());
    }
    Ok(out)
}

/// Base64 back to text, if what it holds is UTF-8.
pub fn base64_decode_text(text: &str) -> Result<String, String> {
    let bytes = base64_decode(text)?;
    String::from_utf8(bytes).map_err(|_| "It decodes, but to bytes that aren't UTF-8 text, so it can't be put in the document. Save it to a file instead.".into())
}

/// JSON as XML, laid out with two spaces of indent.
pub fn json_to_xml(text: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("This isn't JSON: {}", crate::tools::json_error_text(&e)))?;
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    match &value {
        // One member at the top: that's the root element.
        Value::Object(map) if map.len() == 1 && !map.keys().next().is_some_and(|k| k.starts_with('@') || k == "#text") => {
            let (name, inner) = map.iter().next().unwrap();
            if let Value::Array(items) = inner {
                out += "<root>\n";
                for item in items {
                    write_element(&mut out, name, item, 1);
                }
                out += "</root>\n";
            } else {
                write_element(&mut out, name, inner, 0);
            }
        }
        Value::Array(items) => {
            out += "<root>\n";
            for item in items {
                write_element(&mut out, "item", item, 1);
            }
            out += "</root>\n";
        }
        other => write_element(&mut out, "root", other, 0),
    }
    Ok(out)
}

/// A JSON member's name made into an XML element name: letters, digits, `_`, `-`, `.` and `:`, not starting with
/// a digit, a `-` or a `.`.
fn xml_name(name: &str) -> String {
    let mut out: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':') { c } else { '_' }).collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.') || out.to_ascii_lowercase().starts_with("xml") {
        out.insert(0, '_');
    }
    out
}

fn escape(text: &str, in_attribute: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out += "&amp;",
            '<' => out += "&lt;",
            '>' => out += "&gt;",
            '"' if in_attribute => out += "&quot;",
            other => out.push(other),
        }
    }
    out
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn write_element(out: &mut String, name: &str, value: &Value, depth: usize) {
    let indent = "  ".repeat(depth);
    let name = xml_name(name);
    match value {
        Value::Array(items) => {
            for item in items {
                write_element(out, &name, item, depth);
            }
        }
        Value::Object(map) => {
            *out += &format!("{indent}<{name}");
            for (key, inner) in map.iter().filter(|(k, _)| k.len() > 1 && k.starts_with('@')) {
                *out += &format!(" {}=\"{}\"", xml_name(&key[1..]), escape(&scalar_text(inner), true));
            }
            let text = map.get("#text").map(scalar_text);
            let children: Vec<(&String, &Value)> = map.iter().filter(|(k, _)| !(k.len() > 1 && k.starts_with('@')) && *k != "#text").collect();
            if children.is_empty() {
                match text {
                    Some(text) if !text.is_empty() => *out += &format!(">{}</{name}>\n", escape(&text, false)),
                    _ => *out += "/>\n",
                }
                return;
            }
            *out += ">\n";
            if let Some(text) = text {
                *out += &format!("{indent}  {}\n", escape(&text, false));
            }
            for (key, inner) in children {
                write_element(out, key, inner, depth + 1);
            }
            *out += &format!("{indent}</{name}>\n");
        }
        Value::Null => *out += &format!("{indent}<{name}/>\n"),
        scalar => *out += &format!("{indent}<{name}>{}</{name}>\n", escape(&scalar_text(scalar), false)),
    }
}

/// XML as JSON, laid out as Format, Lay out code, JSON would.
pub fn xml_to_json(text: &str) -> Result<String, String> {
    let document = roxmltree::Document::parse_with_options(text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }).map_err(|e| format!("This isn't XML: {e}"))?;
    let root = document.root_element();
    let mut top = Map::new();
    top.insert(root.tag_name().name().to_string(), element_value(root));
    serde_json::to_string_pretty(&Value::Object(top)).map_err(|e| e.to_string())
}

/// Text as the JSON value it reads as: a number, true, false, or else a string. Numbers that wouldn't come back
/// written the same (leading zeros, a `+`, very long ones) stay strings.
fn typed(text: &str) -> Value {
    match text {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if let Ok(number) = serde_json::from_str::<serde_json::Number>(text)
        && number.to_string() == text
    {
        return Value::Number(number);
    }
    Value::String(text.to_string())
}

fn element_value(node: roxmltree::Node) -> Value {
    let mut map = Map::new();
    for attribute in node.attributes() {
        map.insert(format!("@{}", attribute.name()), typed(attribute.value()));
    }
    let mut text = String::new();
    for child in node.children() {
        if child.is_element() {
            let name = child.tag_name().name().to_string();
            let value = element_value(child);
            match map.get_mut(&name) {
                Some(Value::Array(items)) => items.push(value),
                Some(existing) => {
                    let first = existing.take();
                    *existing = Value::Array(vec![first, value]);
                }
                None => {
                    map.insert(name, value);
                }
            }
        } else if child.is_text() {
            text += child.text().unwrap_or("");
        }
    }
    let text = text.trim();
    if map.is_empty() {
        return if text.is_empty() { Value::Null } else { typed(text) };
    }
    if !text.is_empty() {
        map.insert("#text".into(), Value::String(text.to_string()));
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_4648() {
        // RFC 4648, section 10.
        for (plain, coded) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64_encode(plain.as_bytes(), true), coded);
            assert_eq!(base64_decode_text(coded).unwrap(), plain);
        }
    }

    #[test]
    fn base64_wraps_at_76_and_reads_its_own_lines_back() {
        let bytes: Vec<u8> = (0..=255).collect();
        let coded = base64_encode(&bytes, true);
        assert!(coded.lines().all(|line| line.len() <= 76));
        assert_eq!(coded.lines().next().unwrap().len(), 76);
        assert_eq!(base64_decode(&coded).unwrap(), bytes);
        assert!(!base64_encode(&bytes, false).contains('\n'));
    }

    #[test]
    fn base64_takes_url_safe_letters_and_missing_padding() {
        assert_eq!(base64_decode("-_8").unwrap(), [0xFB, 0xFF]);
        assert_eq!(base64_decode("Zm9vYg").unwrap(), b"foob");
    }

    #[test]
    fn base64_says_what_is_wrong() {
        assert!(base64_decode("Zm9v!").unwrap_err().contains('!'));
        assert!(base64_decode("Zm9vY").is_err());
        assert!(base64_decode("Zg==Zg==").is_err());
        assert!(base64_decode_text("/w==").unwrap_err().contains("UTF-8"));
    }

    #[test]
    fn json_becomes_xml_with_attributes_text_and_repeats() {
        let json = r##"{"library": {"@open": true, "name": "Central & West", "book": [{"@id": 1, "#text": "Dune"}, {"@id": 2, "#text": "Emma"}], "note": null, "2nd floor": "yes"}}"##;
        let xml = json_to_xml(json).unwrap();
        assert_eq!(
            xml,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<library open=\"true\">\n  <name>Central &amp; West</name>\n  <book id=\"1\">Dune</book>\n  <book id=\"2\">Emma</book>\n  <note/>\n  <_2nd_floor>yes</_2nd_floor>\n</library>\n"
        );
    }

    #[test]
    fn json_with_no_single_root_gets_one() {
        assert!(json_to_xml("[1, 2]").unwrap().contains("<root>\n  <item>1</item>\n  <item>2</item>\n</root>"));
        assert!(json_to_xml(r#"{"a": 1, "b": 2}"#).unwrap().contains("<root>\n  <a>1</a>\n  <b>2</b>\n</root>"));
        assert!(json_to_xml("{").unwrap_err().starts_with("This isn't JSON"));
    }

    #[test]
    fn xml_becomes_json_and_back() {
        let xml = "<library open=\"true\"><name>Central &amp; West</name><book id=\"1\">Dune</book><book id=\"2\">Emma</book><note/><code>007</code></library>";
        let json = xml_to_json(xml).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"library": {"@open": true, "name": "Central & West", "book": [{"@id": 1, "#text": "Dune"}, {"@id": 2, "#text": "Emma"}], "note": null, "code": "007"}})
        );
        let again: Value = serde_json::from_str(&xml_to_json(&json_to_xml(&json).unwrap()).unwrap()).unwrap();
        assert_eq!(again, value);
        assert!(xml_to_json("<a>").unwrap_err().starts_with("This isn't XML"));
    }
}
