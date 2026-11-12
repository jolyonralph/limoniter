//! Every character set the editor knows, read from one data file, so that opening, saving, the
//! Character table and the palette can't disagree. A byte that has no plain Unicode stand-in is
//! carried through the text as a private character holding that exact byte (see `raw_char`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

const DATA: &str = include_str!("../data/charsets.txt");
/// IBM's mainframe code pages, built by `tools/gen_ebcdic.py`.
const EBCDIC_DATA: &str = include_str!("../data/charsets-ebcdic.txt");
/// More code pages, regional, Macintosh and workstation, built by `tools/gen_codepages.py`.
const MORE_DATA: &str = include_str!("../data/charsets-more.txt");
/// More home and personal computers' sets, built by `tools/gen_retro.py`.
const RETRO_DATA: &str = include_str!("../data/charsets-retro.txt");

/// Bytes kept exactly as they were live here, in the last private-use row of Unicode.
/// A file that really contains these characters has them kept as their separate bytes instead.
const RAW_BASE: u32 = 0x10FF00;

pub fn raw_char(byte: u8) -> char {
    char::from_u32(RAW_BASE + u32::from(byte)).unwrap_or('\u{FFFD}')
}

pub fn raw_byte(c: char) -> Option<u8> {
    let value = c as u32;
    (value >= RAW_BASE).then(|| (value - RAW_BASE) as u8)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Printable,
    Control,
    Undefined,
    /// What it shows depends on a mode, a font or a program: user-defined characters, BASIC keywords.
    StateDependent,
}

impl Kind {
    pub fn describe(self) -> &'static str {
        match self {
            Kind::Printable => "Printable",
            Kind::Control => "Control",
            Kind::Undefined => "Undefined",
            Kind::StateDependent => "Depends on display state",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub unicode: Option<char>,
    pub kind: Kind,
    pub inverse: bool,
    pub name: String,
}

pub struct Table {
    pub title: String,
    pub entries: Vec<Entry>,
    reverse: HashMap<char, u8>,
}

impl Table {
    pub fn byte_of(&self, c: char) -> Option<u8> {
        self.reverse.get(&c).copied()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnicodeForm {
    Utf8,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
}

impl UnicodeForm {
    pub fn bom(self) -> &'static [u8] {
        match self {
            UnicodeForm::Utf8 => &[0xEF, 0xBB, 0xBF],
            UnicodeForm::Utf16Le => &[0xFF, 0xFE],
            UnicodeForm::Utf16Be => &[0xFE, 0xFF],
            UnicodeForm::Utf32Le => &[0xFF, 0xFE, 0x00, 0x00],
            UnicodeForm::Utf32Be => &[0x00, 0x00, 0xFE, 0xFF],
        }
    }
}

/// The East Asian sets where a character takes one or more bytes, read and written by `encoding_rs`,
/// which follows the WHATWG Encoding Standard's tables.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MultiForm {
    ShiftJis,
    EucJp,
    Iso2022Jp,
    Gbk,
    Gb18030,
    Big5,
    EucKr,
    /// UTF-7 (RFC 2152), for mail and news; read and written here, not by encoding_rs. RFC 2152's optional
    /// characters (! " # and the like) are written as themselves, as Python's encoder does.
    Utf7,
    /// The same, with the optional characters written in base64, as glibc's iconv does.
    Utf7Strict,
}

impl MultiForm {
    /// The encoding_rs encoding, for the sets it reads. UTF-7 is read and written in `codec`.
    pub fn encoding(self) -> &'static encoding_rs::Encoding {
        match self {
            MultiForm::Utf7 | MultiForm::Utf7Strict => encoding_rs::UTF_8,
            MultiForm::ShiftJis => encoding_rs::SHIFT_JIS,
            MultiForm::EucJp => encoding_rs::EUC_JP,
            MultiForm::Iso2022Jp => encoding_rs::ISO_2022_JP,
            MultiForm::Gbk => encoding_rs::GBK,
            MultiForm::Gb18030 => encoding_rs::GB18030,
            MultiForm::Big5 => encoding_rs::BIG5,
            MultiForm::EucKr => encoding_rs::EUC_KR,
        }
    }

    /// Whether what a byte means depends on escape sequences earlier in the line.
    pub fn is_stateful(self) -> bool {
        matches!(self, MultiForm::Iso2022Jp | MultiForm::Utf7 | MultiForm::Utf7Strict)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    Unicode(UnicodeForm),
    /// One or more bytes a character, with no table of 256 entries.
    Multi(MultiForm),
    Bytes,
}

/// How what's on screen depends on the bytes before it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Model {
    Plain,
    /// 0x0E and 0x8E switch the character set from there on.
    Petscii,
    /// Each row starts in alphanumerics; controls switch to mosaics in that row.
    Teletext,
    /// The same, read through the SAA5050, where bit 7 of a screen byte doesn't change what it shows.
    Mode7,
}

pub struct Charset {
    pub id: String,
    pub name: String,
    pub group: String,
    pub source: String,
    pub version: String,
    pub scope: String,
    pub family: Family,
    pub model: Model,
    /// Every byte stays a byte in the text, because what it means depends on state.
    pub keep_bytes: bool,
    /// An IBM mainframe set, whose line endings are its own bytes: NL 0x15, LF 0x25 and CR 0x0D.
    pub ebcdic: bool,
    pub tables: Vec<Table>,
    /// What each byte becomes in the text when it isn't a line ending.
    pub decoded: [char; 256],
    labels: Vec<String>,
    wide: [u8; 256],
}

const C0_NAMES: [&str; 32] = [
    "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "BEL", "BS", "HT", "LF", "VT", "FF", "CR", "SO", "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC", "FS", "GS",
    "RS", "US",
];
const C1_NAMES: [&str; 32] = [
    "PAD", "HOP", "BPH", "NBH", "IND", "NEL", "SSA", "ESA", "HTS", "HTJ", "VTS", "PLD", "PLU", "RI", "SS2", "SS3", "DCS", "PU1", "PU2", "STS", "CCH", "MW", "SPA", "EPA", "SOS", "SGCI", "SCI", "CSI", "ST", "OSC",
    "PM", "APC",
];
/// The longest a control's label may be on screen, brackets included.
const MAX_LABEL: usize = 24;

/// The usual short name of a control character, from ISO 6429.
pub fn control_name(value: u32) -> Option<&'static str> {
    match value {
        0..=0x1F => Some(C0_NAMES[value as usize]),
        0x7F => Some("DEL"),
        0x80..=0x9F => Some(C1_NAMES[value as usize - 0x80]),
        _ => None,
    }
}

/// The label a real control character gets in a Unicode file, such as `[ESC]`.
pub fn control_label(c: char) -> String {
    control_name(c as u32).map_or_else(|| format!("[U+{:04X}]", c as u32), |name| format!("[{name}]"))
}

impl Charset {
    pub fn is_unicode(&self) -> bool {
        matches!(self.family, Family::Unicode(_))
    }

    /// The text holds real characters, as in Unicode and the multi-byte sets, rather than bytes read through
    /// a table of 256. Exact bytes the set can't read still travel as private characters.
    pub fn holds_characters(&self) -> bool {
        !matches!(self.family, Family::Bytes)
    }

    #[cfg(test)]
    pub fn multi(&self) -> Option<MultiForm> {
        match self.family {
            Family::Multi(form) => Some(form),
            _ => None,
        }
    }

    pub fn bom(&self) -> Option<&'static [u8]> {
        match self.family {
            Family::Unicode(form) => Some(form.bom()),
            Family::Multi(_) | Family::Bytes => None,
        }
    }

    /// How many byte values a table shows: strict ASCII stops at 127.
    pub fn row_count(&self) -> usize {
        if self.id == "us-ascii" { 128 } else { 256 }
    }

    pub fn table(&self, state: usize) -> Option<&Table> {
        self.tables.get(state).or_else(|| self.tables.first())
    }

    pub fn entry(&self, state: usize, byte: u8) -> Option<&Entry> {
        self.table(state).map(|table| &table.entries[usize::from(byte)])
    }

    /// The token shown for a byte in the source view when it has no character to draw.
    pub fn label(&self, byte: u8) -> &str {
        self.labels.get(usize::from(byte)).map_or("", String::as_str)
    }

    pub fn widths(&self) -> &[u8; 256] {
        &self.wide
    }

    /// A character for the text: itself where it can stand for its byte, or else the byte.
    /// Anything the set can't hold is left as it is, to be reported before saving.
    pub fn for_text(&self, c: char, state: usize) -> char {
        if raw_byte(c).is_some() {
            return c;
        }
        let Some(table) = self.table(state) else {
            return c;
        };
        let found = table.byte_of(c).or_else(|| {
            // A machine with no lower case takes typing as its capitals, as its own keyboard does.
            if self.keep_bytes && c.is_lowercase() { c.to_uppercase().next().and_then(|upper| table.byte_of(upper)) } else { None }
        });
        match found {
            Some(byte) if self.keep_bytes => raw_char(byte),
            Some(byte) if self.decoded[usize::from(byte)] == c => c,
            Some(byte) => raw_char(byte),
            None => c,
        }
    }
}

fn hex_u32(text: &str) -> Option<u32> {
    u32::from_str_radix(text, 16).ok()
}

fn blank_entries() -> Vec<Entry> {
    vec![Entry { unicode: None, kind: Kind::Undefined, inverse: false, name: String::new() }; 256]
}

fn finish_table(title: String, entries: Vec<Entry>, written_as: &[(char, u8)]) -> Table {
    let mut reverse = HashMap::new();
    // Characters first, so a control sharing a value never takes its place.
    for wanted in [Kind::Printable, Kind::Control] {
        for (byte, entry) in entries.iter().enumerate() {
            if entry.kind == wanted
                && let Some(c) = entry.unicode
            {
                reverse.entry(c).or_insert(byte as u8);
            }
        }
    }
    for (c, byte) in written_as {
        reverse.insert(*c, *byte);
    }
    Table { title, entries, reverse }
}

fn finish_charset(mut charset: Charset) -> Charset {
    let mut decoded = ['\0'; 256];
    let mut labels = Vec::with_capacity(256);
    let mut wide = [1u8; 256];
    for value in 0..=255u8 {
        let index = usize::from(value);
        let entry = charset.tables.first().map(|table| &table.entries[index]);
        let c = entry.and_then(|e| e.unicode);
        let keeps_character = !charset.keep_bytes
            && match entry.map(|e| e.kind) {
                Some(Kind::Printable) => c.is_some_and(|c| charset.tables[0].byte_of(c) == Some(value) && raw_byte(c).is_none()),
                // A real tab byte stays a tab, so that tabs edit and line up as tabs.
                Some(Kind::Control) => value == 9 && c == Some('\t'),
                _ => false,
            };
        decoded[index] = if keeps_character { c.unwrap_or('\0') } else { raw_char(value) };
        let label = make_label(value, entry, charset.holds_characters(), charset.keep_bytes);
        wide[index] = if label.is_empty() { 1 } else { label.chars().count().min(255) as u8 };
        labels.push(label);
    }
    charset.decoded = decoded;
    charset.labels = labels;
    charset.wide = wide;
    charset
}

/// Empty for a byte that shows as a character.
fn make_label(value: u8, entry: Option<&Entry>, unicode: bool, machine_codes: bool) -> String {
    let hex = || format!("[0x{value:02X}]");
    let Some(entry) = entry else {
        return if unicode { hex() } else { String::new() };
    };
    match entry.kind {
        Kind::Printable => String::new(),
        Kind::Undefined => hex(),
        Kind::StateDependent => {
            let word = entry.name.strip_prefix("BASIC keyword ");
            word.map_or_else(hex, short_label)
        }
        Kind::Control => {
            let is_itself = entry.unicode.is_some_and(|c| c as u32 == u32::from(value));
            if !entry.name.is_empty() {
                short_label(&entry.name)
            } else if is_itself && !machine_codes {
                // Only a real ISO control set has these names; a machine's own code with no name just shows its value.
                control_name(u32::from(value)).map_or_else(hex, |name| format!("[{name}]"))
            } else {
                hex()
            }
        }
    }
}

fn short_label(name: &str) -> String {
    let upper = name.to_uppercase();
    let body: String = upper.chars().take(MAX_LABEL - 2).collect();
    format!("[{body}]")
}

fn unicode_charset(id: &str, name: &str, form: UnicodeForm) -> Charset {
    finish_charset(Charset {
        id: id.to_string(),
        name: name.to_string(),
        group: "Unicode".to_string(),
        source: "The Unicode Standard".to_string(),
        version: "Rust's own Unicode handling".to_string(),
        scope: "Complete. Invalid bytes are kept exactly and shown as tokens.".to_string(),
        family: Family::Unicode(form),
        model: Model::Plain,
        keep_bytes: false,
        ebcdic: false,
        tables: Vec::new(),
        decoded: ['\0'; 256],
        labels: Vec::new(),
        wide: [1; 256],
    })
}

fn multi_charset(id: &str, name: &str, form: MultiForm, scope: &str) -> Charset {
    let mut set = unicode_charset(id, name, UnicodeForm::Utf8);
    set.family = Family::Multi(form);
    set.group = "East Asian (multi-byte)".to_string();
    set.source = "The WHATWG Encoding Standard's tables, through the encoding_rs crate".to_string();
    set.version = "encoding_rs 0.8".to_string();
    set.scope = scope.to_string();
    set
}

/// Reads charset definitions. A bad line is skipped and noted; it never stops the rest loading.
pub fn parse_definitions(text: &str, problems: &mut Vec<String>) -> Vec<Charset> {
    let mut sets: Vec<Charset> = Vec::new();
    let mut current: Option<Charset> = None;
    let mut table: Option<(String, Vec<Entry>, Vec<(char, u8)>)> = None;
    let close_table = |table: &mut Option<(String, Vec<Entry>, Vec<(char, u8)>)>, current: &mut Option<Charset>| {
        if let (Some((title, entries, written)), Some(charset)) = (table.take(), current.as_mut()) {
            charset.tables.push(finish_table(title, entries, &written));
        }
    };
    for (number, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(id) = line.strip_prefix("@charset ") {
            close_table(&mut table, &mut current);
            if let Some(done) = current.take() {
                sets.push(finish_charset(done));
            }
            current = Some(Charset {
                id: id.trim().to_string(),
                name: id.trim().to_string(),
                group: "Others".to_string(),
                source: String::new(),
                version: String::new(),
                scope: String::new(),
                family: Family::Bytes,
                model: Model::Plain,
                keep_bytes: false,
                ebcdic: false,
                tables: Vec::new(),
                decoded: ['\0'; 256],
                labels: Vec::new(),
                wide: [1; 256],
            });
            continue;
        }
        if let Some(title) = line.strip_prefix("@table ") {
            close_table(&mut table, &mut current);
            table = Some((title.trim().to_string(), blank_entries(), Vec::new()));
            continue;
        }
        if let Some((_, entries, written)) = &mut table {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.first() == Some(&"<") {
                let target = parts.get(1).and_then(|p| hex_u32(p)).and_then(char::from_u32);
                let byte = parts.get(2).and_then(|p| hex_u32(p));
                if let (Some(c), Some(byte)) = (target, byte) {
                    written.push((c, byte as u8));
                }
                continue;
            }
            let byte = parts.first().and_then(|p| hex_u32(p)).filter(|b| *b < 256);
            let kind = parts.get(2).copied().unwrap_or("");
            let Some(byte) = byte else {
                problems.push(format!("Line {} isn't a table row: {line}", number + 1));
                continue;
            };
            let unicode = parts.get(1).and_then(|p| hex_u32(p)).and_then(char::from_u32);
            entries[byte as usize] = Entry {
                unicode,
                kind: match kind.chars().next() {
                    Some('p') => Kind::Printable,
                    Some('c') => Kind::Control,
                    Some('s') => Kind::StateDependent,
                    _ => Kind::Undefined,
                },
                inverse: kind.contains('i'),
                name: parts.get(3).copied().unwrap_or("").to_string(),
            };
            continue;
        }
        if let (Some(charset), Some((key, value))) = (current.as_mut(), line.split_once(": ")) {
            match key {
                "name" => charset.name = value.to_string(),
                "group" => charset.group = value.to_string(),
                "source" => charset.source = value.to_string(),
                "version" => charset.version = value.to_string(),
                "scope" => charset.scope = value.to_string(),
                "keep_bytes" => charset.keep_bytes = value.trim() == "true",
                "ebcdic" => charset.ebcdic = value.trim() == "true",
                "model" => {
                    charset.model = match value.trim() {
                        "petscii" => Model::Petscii,
                        "teletext" => Model::Teletext,
                        "mode7" => Model::Mode7,
                        _ => Model::Plain,
                    };
                }
                _ => {}
            }
        }
    }
    close_table(&mut table, &mut current);
    if let Some(done) = current.take() {
        sets.push(finish_charset(done));
    }
    sets.retain(|set| {
        let usable = !set.tables.is_empty();
        if !usable {
            problems.push(format!("{} has no table", set.id));
        }
        usable
    });
    sets
}

/// More character sets can be added by dropping definition files in this folder.
pub fn user_folder() -> Option<PathBuf> {
    crate::settings::data_folder().map(|dir| dir.join("charsets"))
}

struct Registry {
    sets: Vec<&'static Charset>,
}

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut problems = Vec::new();
        let mut sets = vec![
            unicode_charset("utf-8", "UTF-8", UnicodeForm::Utf8),
            unicode_charset("utf-16le", "UTF-16 LE", UnicodeForm::Utf16Le),
            unicode_charset("utf-16be", "UTF-16 BE", UnicodeForm::Utf16Be),
            unicode_charset("utf-32le", "UTF-32 LE", UnicodeForm::Utf32Le),
            unicode_charset("utf-32be", "UTF-32 BE", UnicodeForm::Utf32Be),
        ];
        sets.extend(parse_definitions(DATA, &mut problems));
        sets.extend(parse_definitions(EBCDIC_DATA, &mut problems));
        sets.extend(parse_definitions(MORE_DATA, &mut problems));
        sets.extend(parse_definitions(RETRO_DATA, &mut problems));
        let exact = "Complete. A byte sequence that wouldn't come back the same when saved is kept as its exact bytes and shown as tokens.";
        sets.extend([
            multi_charset("shift-jis", "Shift-JIS (Japanese, Windows code page 932)", MultiForm::ShiftJis, exact),
            multi_charset("euc-jp", "EUC-JP (Japanese, Unix)", MultiForm::EucJp, exact),
            multi_charset("iso-2022-jp", "ISO-2022-JP (Japanese, mail and news)", MultiForm::Iso2022Jp, "Read a line at a time, as each line returns to ASCII. A line that wouldn't come back the same is kept as its exact bytes."),
            multi_charset("gbk", "GBK (Simplified Chinese, Windows code page 936)", MultiForm::Gbk, exact),
            multi_charset("gb18030", "GB 18030 (Chinese, all of Unicode)", MultiForm::Gb18030, exact),
            multi_charset("big5", "Big5 (Traditional Chinese, with HKSCS)", MultiForm::Big5, exact),
            multi_charset("euc-kr", "EUC-KR (Korean, Windows code page 949)", MultiForm::EucKr, exact),
        ]);
        let mut utf7 = multi_charset("utf-7", "UTF-7 (mail and news)", MultiForm::Utf7, "RFC 2152, read a line at a time and written as Python's encoder writes it. A line that wouldn't come back the same is kept as its exact bytes.");
        utf7.group = "Unicode".to_string();
        utf7.source = "RFC 2152".to_string();
        utf7.version = "Written for Limoniter".to_string();
        let mut strict = multi_charset("utf-7-strict", "UTF-7, optional characters encoded (as iconv writes it)", MultiForm::Utf7Strict, "RFC 2152 as glibc's iconv writes it, with ! \" # and the other optional characters in base64. A line that wouldn't come back the same is kept as its exact bytes.");
        strict.group = utf7.group.clone();
        strict.source = utf7.source.clone();
        strict.version = utf7.version.clone();
        sets.push(utf7);
        sets.push(strict);
        if let Some(folder) = user_folder()
            && let Ok(files) = std::fs::read_dir(folder)
        {
            for file in files.flatten() {
                if let Ok(text) = std::fs::read_to_string(file.path()) {
                    for set in parse_definitions(&text, &mut problems) {
                        if !sets.iter().any(|known| known.id == set.id) {
                            sets.push(set);
                        }
                    }
                }
            }
        }
        Registry { sets: sets.into_iter().map(|set| &*Box::leak(Box::new(set))).collect() }
    })
}

pub fn all() -> &'static [&'static Charset] {
    &registry().sets
}

pub fn by_id(id: &str) -> Option<&'static Charset> {
    all().iter().copied().find(|set| set.id == id)
}

pub fn utf8() -> &'static Charset {
    by_id("utf-8").expect("UTF-8 is always defined")
}

/// A charset known to be in the data file; a missing one is a build mistake, found by the tests.
pub fn known(id: &str) -> &'static Charset {
    by_id(id).unwrap_or_else(utf8)
}

/// The group names, in the order they first appear.
pub fn groups() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::new();
    for set in all() {
        if !names.contains(&set.group.as_str()) {
            names.push(&set.group);
        }
    }
    names
}

/// A character's name, for the Character table and palette.
pub fn char_name(c: char) -> String {
    if let Some(name) = control_name(c as u32) {
        return format!("control {name}");
    }
    unicode_names2::name(c).map_or_else(|| format!("U+{:04X}", c as u32), |name| name.to_string())
}

/// Follows the character-set state as bytes go by, for what the screen and the table should show.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellState {
    model: Model,
    /// The set a stream starts in, or a row's starting alphanumeric set.
    pub table: usize,
    separated: bool,
    start: usize,
    /// Odd-parity data: bit 7 is the parity bit, so it isn't part of the code.
    parity: bool,
}

impl CellState {
    pub fn new(model: Model, start: usize, parity: bool) -> Self {
        Self { model, table: start, separated: false, start, parity }
    }

    /// Teletext attributes last for one row.
    pub fn next_row(&mut self) {
        if matches!(self.model, Model::Teletext | Model::Mode7) {
            self.table = self.start;
            self.separated = false;
        }
    }

    /// The byte's meaning here, with bit 7 taken out only where the format says it isn't data.
    pub fn code(&self, byte: u8) -> u8 {
        if self.parity { byte & 0x7F } else { byte }
    }

    /// Where a control takes effect: set-after, so a control cell itself uses the old state.
    pub fn advance(&mut self, byte: u8) {
        match self.model {
            Model::Plain => {}
            Model::Petscii => match byte {
                0x0E => self.table = 1,
                0x8E => self.table = 0,
                _ => {}
            },
            Model::Teletext | Model::Mode7 => {
                let code = if self.model == Model::Mode7 { byte & 0x7F } else { self.code(byte) };
                match code {
                    0x00..=0x07 => self.table = 0,
                    0x10..=0x17 => self.table = if self.separated { 2 } else { 1 },
                    0x19 => {
                        self.separated = false;
                        if self.table != 0 {
                            self.table = 1;
                        }
                    }
                    0x1A => {
                        self.separated = true;
                        if self.table != 0 {
                            self.table = 2;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// What to look a byte up with, in a set that shows through bit 7 or not.
    pub fn lookup(&self, byte: u8) -> u8 {
        if self.model == Model::Teletext { self.code(byte) } else { byte }
    }

    /// Mosaic tables only hold mosaics in their own columns; capitals still come from the alphanumeric set.
    pub fn table_for(&self, charset: &Charset, byte: u8) -> usize {
        let code = byte & 0x7F;
        if matches!(charset.model, Model::Teletext | Model::Mode7) && self.table != 0 && (0x40..=0x5F).contains(&code) {
            return 0;
        }
        self.table.min(charset.tables.len().saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_set_is_there_with_a_source() {
        for id in [
            "windows-1252", "iso-8859-1", "iso-8859-15", "cp437", "cp850", "mac-roman", "amiga-text", "petscii-c64", "petscii-vic20", "petscii-pet", "screen-c64", "atari-st", "atascii", "zx-spectrum", "bbc-micro",
            "bbc-mode7", "teletext-en", "teletext-de", "msx", "msx-jp", "us-ascii", "utf-8", "utf-16le", "utf-16be", "utf-32le", "utf-32be",
        ] {
            let set = by_id(id).unwrap_or_else(|| panic!("{id} is missing"));
            assert!(!set.source.is_empty() && !set.version.is_empty() && !set.scope.is_empty(), "{id} says where it came from");
        }
    }

    #[test]
    fn the_same_byte_means_what_each_set_says() {
        let csi = known("amiga-text");
        assert_eq!(csi.label(0x9B), "[CSI]");
        let eol = known("atascii");
        assert_eq!(eol.entry(0, 0x9B).unwrap().name, "end of line");
        assert_eq!(eol.entry(0, 0x9B).unwrap().kind, Kind::Control);
        assert_eq!(known("windows-1252").entry(0, 0x9B).unwrap().unicode, Some('\u{203A}'));
    }

    #[test]
    fn dos_and_windows_read_the_upper_bytes_differently() {
        assert_eq!(known("cp437").entry(0, 0xE9).unwrap().unicode, Some('\u{398}'));
        assert_eq!(known("windows-1252").entry(0, 0xE9).unwrap().unicode, Some('\u{E9}'));
        assert_eq!(known("cp850").entry(0, 0xE9).unwrap().unicode, Some('\u{DA}'));
    }

    #[test]
    fn switching_petscii_state_changes_the_glyphs() {
        let set = known("petscii-c64");
        assert_eq!(set.entry(0, 0x41).unwrap().unicode, Some('A'));
        assert_eq!(set.entry(1, 0x41).unwrap().unicode, Some('a'));
        assert_eq!(set.entry(0, 0xC1).unwrap().unicode, Some('\u{2660}'));
        assert_eq!(set.entry(1, 0xC1).unwrap().unicode, Some('A'));
        let mut state = CellState::new(set.model, 0, false);
        state.advance(0x0E);
        assert_eq!(state.table, 1);
        state.advance(0x8E);
        assert_eq!(state.table, 0);
    }

    #[test]
    fn screen_codes_are_not_petscii() {
        assert_eq!(known("screen-c64").entry(0, 0x01).unwrap().unicode, Some('A'));
        assert_ne!(known("petscii-c64").entry(0, 0x01).unwrap().unicode, Some('A'));
    }

    #[test]
    fn teletext_mosaics_last_for_the_row_and_national_sets_differ() {
        let set = known("teletext-en");
        let mut state = CellState::new(set.model, 0, false);
        state.advance(0x11);
        assert_eq!(state.table_for(set, 0x21), 1);
        assert_eq!(state.table_for(set, 0x41), 0, "capitals stay alphanumeric in mosaic mode");
        state.advance(0x1A);
        assert_eq!(state.table_for(set, 0x21), 2);
        state.next_row();
        assert_eq!(state.table_for(set, 0x21), 0);
        assert_eq!(known("teletext-en").entry(0, 0x23).unwrap().unicode, Some('\u{A3}'));
        assert_ne!(known("teletext-de").entry(0, 0x5B).unwrap().unicode, known("teletext-en").entry(0, 0x5B).unwrap().unicode);
    }

    #[test]
    fn parity_is_only_stripped_when_the_format_has_it() {
        let set = known("teletext-en");
        assert_eq!(CellState::new(set.model, 0, false).code(0xC1), 0xC1);
        assert_eq!(CellState::new(set.model, 0, true).code(0xC1), 0x41);
    }

    #[test]
    fn duplicates_and_undefined_bytes_are_kept_as_bytes() {
        let set = known("petscii-c64");
        assert!(raw_byte(set.decoded[0x61]).is_some());
        let dos = known("cp437");
        assert_eq!(dos.decoded[0x41], 'A');
        assert_eq!(known("windows-1252").decoded[0x81], raw_char(0x81));
        assert_eq!(known("windows-1252").label(0x81), "[0x81]");
        assert_eq!(known("us-ascii").decoded[0xE9], raw_char(0xE9));
        assert_eq!(known("us-ascii").row_count(), 128);
        assert_eq!(known("petscii-c64").label(0x80), "[0x80]", "PETSCII doesn't define this, so it gets no ISO name");
    }

    #[test]
    fn every_defined_character_finds_its_way_back_to_a_byte() {
        for set in all().iter().filter(|set| !set.holds_characters()) {
            for (index, table) in set.tables.iter().enumerate() {
                for (byte, entry) in table.entries.iter().enumerate() {
                    if entry.kind == Kind::Printable
                        && let Some(c) = entry.unicode
                    {
                        let back = table.byte_of(c);
                        assert!(back.is_some(), "{} table {index} byte {byte:02X}", set.id);
                    }
                }
            }
        }
    }

    #[test]
    fn typing_becomes_the_right_byte_for_the_set() {
        let pet = known("petscii-c64");
        assert_eq!(pet.for_text('A', 0), raw_char(0x41));
        assert_eq!(pet.for_text('a', 0), raw_char(0x41), "no lower case in this state, so it types the capital");
        assert_eq!(pet.for_text('a', 1), raw_char(0x41));
        assert_eq!(pet.for_text('A', 1), raw_char(0xC1));
        let dos = known("cp437");
        assert_eq!(dos.for_text('\u{398}', 0), '\u{398}');
        assert_eq!(dos.for_text('\u{1B}', 0), raw_char(0x1B));
        assert_eq!(dos.for_text('\u{20AC}', 0), '\u{20AC}', "left alone, to be reported before saving");
    }

    #[test]
    fn definitions_can_be_added_as_data() {
        let mut problems = Vec::new();
        let text = "@charset x-test\nname: Test\ngroup: Tests\nsource: here\nversion: 1\nscope: small\nmodel: none\nkeep_bytes: false\n@table Text\n41\t0391\tp\tALPHA\n1B\t001B\tc\t\nnot a row\n";
        let sets = parse_definitions(text, &mut problems);
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].decoded[0x41], '\u{391}');
        assert_eq!(sets[0].label(0x1B), "[ESC]");
        assert_eq!(sets[0].label(0x42), "[0x42]", "a byte it doesn't define is never guessed at");
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn ebcdic_pages_load_from_ibms_tables() {
        let mut problems = Vec::new();
        let sets = parse_definitions(EBCDIC_DATA, &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(sets.len(), 22);
        assert!(sets.iter().all(|set| set.ebcdic && set.group.starts_with("EBCDIC")));
        let us = known("ebcdic-037");
        assert_eq!(us.decoded[0xC1], 'A');
        assert_eq!(us.decoded[0x40], ' ');
        assert_eq!(us.decoded[0xF0], '0');
        assert_eq!(us.decoded[0x4A], '\u{A2}');
        assert_eq!(us.label(0x15), "[NL]");
        assert_eq!(us.label(0x25), "[LF]");
        assert_eq!(known("ebcdic-1140").decoded[0x9F], '\u{20AC}', "the euro pages have the euro where 037 has its currency sign");
        assert_eq!(known("ebcdic-273").decoded[0x4A], '\u{C4}', "German has A umlaut where US has the cent sign");
        assert_eq!(known("ebcdic-1047").decoded[0xAD], '[', "1047 moves the brackets for C and Unix");
        assert_eq!(known("ebcdic-037").decoded[0xBA], '[');
        assert!(!known("windows-1252").ebcdic);
        // The table the hex pane uses agrees with IBM's.
        for byte in 0..=255u8 {
            if let Some(c) = us.entry(0, byte).and_then(|e| e.unicode) {
                assert_eq!(crate::ebcdic::char_of(byte), c, "{byte:02X}");
            }
        }
    }

    #[test]
    fn the_further_code_pages_load_and_read_as_their_sources_say() {
        let mut problems = Vec::new();
        let sets = parse_definitions(MORE_DATA, &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(sets.len(), 29);
        for (id, byte, c) in [
            ("viscii", 0x02, '\u{1EB2}'),
            ("nextstep", 0x80, '\u{A0}'),
            ("nextstep", 0x41, 'A'),
            ("mac-symbol", 0x61, '\u{3B1}'),
            ("hp-roman8", 0xA1, '\u{C0}'),
            ("tis-620", 0xA1, '\u{E01}'),
            ("dec-mcs", 0xD7, '\u{152}'),
            ("cp1125", 0xF2, '\u{490}'),
            ("mac-celtic", 0x0D, '\r'),
        ] {
            assert_eq!(known(id).entry(0, byte).and_then(|e| e.unicode), Some(c), "{id} {byte:02X}");
        }
        // Mac Thai has bytes that stand for two characters; they're kept as their bytes.
        let thai = known("mac-thai");
        assert!(thai.tables[0].entries.iter().any(|e| e.kind == Kind::StateDependent && e.unicode.is_none()));
        assert!(groups().contains(&"Workstations and handhelds"));
    }

    #[test]
    fn the_further_home_computer_sets_load_with_their_inverse_forms() {
        let mut problems = Vec::new();
        let sets = parse_definitions(RETRO_DATA, &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(sets.len(), 30);
        // Apple II text is high-bit ASCII: typing A writes 0xC1, and the low byte with A in it is the inverse form.
        let apple = known("apple2e");
        assert_eq!(apple.decoded[0xC1], 'A');
        assert_eq!(apple.for_text('A', 0), 'A');
        assert_eq!(apple.tables[0].byte_of('A'), Some(0xC1));
        assert!(apple.entry(0, 0x01).is_some_and(|e| e.inverse && e.unicode == Some('A')));
        // The ZX81 isn't ASCII, and its top half is the bottom half inverted.
        let zx81 = known("zx81");
        assert_eq!(zx81.entry(0, 0x26).and_then(|e| e.unicode), Some('A'));
        assert!(zx81.entry(0, 0xA6).is_some_and(|e| e.inverse && e.unicode == Some('A')));
        assert_eq!(zx81.tables[0].byte_of('A'), Some(0x26));
        assert!(known("kamenicky").entry(0, 0x80).is_some_and(|e| e.unicode == Some('\u{10C}')), "Kamenický starts with C caron");
        for id in ["trs80-m1", "oric", "sharp-mz-eu", "amstrad-cpc", "ti994a", "riscos", "koi7"] {
            assert!(by_id(id).is_some(), "{id}");
        }
    }

    #[test]
    fn the_east_asian_sets_are_there_and_hold_characters() {
        for id in ["shift-jis", "euc-jp", "iso-2022-jp", "gbk", "gb18030", "big5", "euc-kr", "utf-7", "utf-7-strict"] {
            let set = by_id(id).unwrap_or_else(|| panic!("{id} is missing"));
            assert!(set.holds_characters() && !set.is_unicode() && set.bom().is_none() && set.multi().is_some(), "{id}");
            assert!(!set.source.is_empty() && !set.scope.is_empty());
        }
        assert!(groups().contains(&"East Asian (multi-byte)"));
        assert!(!known("cp437").holds_characters());
    }

    #[test]
    fn the_data_file_loads_without_problems() {
        let mut problems = Vec::new();
        let sets = parse_definitions(DATA, &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert!(sets.len() > 90);
    }
}
