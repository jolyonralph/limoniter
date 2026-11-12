//! Turning a file's bytes into lines of text and back, without changing anything by accident.
//!
//! The settings are kept separate from one another: character set, what counts as a line ending,
//! byte order mark, how it's displayed, and the container it's stored in. A preset only fills them in.
//! Bytes that have no plain place in Unicode travel through the text as exact bytes (`charset::raw_char`),
//! and each line keeps its own ending, so opening and saving gives back the same file, byte for byte.

use crate::charset::{self, CellState, Charset, Family, MultiForm, UnicodeForm, raw_byte, raw_char};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

/// Files larger than this are refused, because they are edited in memory.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;
/// How many problems are listed in full; the total is still counted.
pub const MAX_LISTED_PROBLEMS: usize = 500;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Eol {
    CrLf,
    Lf,
    Cr,
    /// ATASCII's end of line, byte 0x9B.
    Atascii,
    /// EBCDIC's new line, byte 0x15, as z/OS Unix text uses.
    Nl,
    /// Fixed-width rows have nothing between them.
    None,
}

impl Eol {
    /// The names shown in the status bar.
    pub fn name(self) -> &'static str {
        match self {
            Eol::CrLf => "CRLF",
            Eol::Lf => "LF",
            Eol::Cr => "CR",
            Eol::Atascii => "ATASCII EOL",
            Eol::Nl => "NL",
            Eol::None => "Rows",
        }
    }

    pub fn long_name(self) -> &'static str {
        match self {
            Eol::CrLf => "CRLF (DOS, Windows)",
            Eol::Lf => "LF (Unix, Linux, macOS)",
            Eol::Cr => "CR (classic Macintosh, Commodore)",
            Eol::Atascii => "ATASCII end of line (byte 0x9B)",
            Eol::Nl => "NL (EBCDIC new line, byte 0x15)",
            Eol::None => "None (fixed-width rows)",
        }
    }

    /// How many characters it counts as in the original's character count.
    pub fn char_count(self) -> u64 {
        match self {
            Eol::CrLf => 2,
            Eol::None => 0,
            _ => 1,
        }
    }
}

/// What the file's line breaks are taken to be. Nothing else ever breaks a line:
/// not NUL, Ctrl-Z or Unicode NEL, and no display control, unless a profile here says so.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NewlinePolicy {
    /// CR, LF and CRLF, each one kept as it was.
    Standard,
    /// Only CR, for machines where LF is just another code.
    CrOnly,
    /// Only byte 0x9B; CR and LF bytes are ordinary data.
    AtasciiEol,
    /// No line endings at all: the file is rows of a fixed width.
    FixedRows { width: usize, parity: bool },
}

impl NewlinePolicy {
    pub const CHOICES: [NewlinePolicy; 6] = [
        NewlinePolicy::Standard,
        NewlinePolicy::CrOnly,
        NewlinePolicy::AtasciiEol,
        NewlinePolicy::FixedRows { width: 40, parity: false },
        NewlinePolicy::FixedRows { width: 40, parity: true },
        NewlinePolicy::FixedRows { width: 80, parity: false },
    ];

    pub fn name(self) -> String {
        match self {
            NewlinePolicy::Standard => "CR, LF and CRLF (and NL in EBCDIC)".to_string(),
            NewlinePolicy::CrOnly => "CR only".to_string(),
            NewlinePolicy::AtasciiEol => "ATASCII EOL (0x9B) only".to_string(),
            NewlinePolicy::FixedRows { width, parity: false } => format!("Raw rows of {width} cells"),
            NewlinePolicy::FixedRows { width, parity: true } => format!("Raw rows of {width} cells, odd parity"),
        }
    }

    pub fn key(self) -> String {
        match self {
            NewlinePolicy::Standard => "standard".to_string(),
            NewlinePolicy::CrOnly => "cr-only".to_string(),
            NewlinePolicy::AtasciiEol => "atascii".to_string(),
            NewlinePolicy::FixedRows { width, parity } => format!("rows-{width}{}", if parity { "-parity" } else { "" }),
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "standard" => Some(NewlinePolicy::Standard),
            "cr-only" => Some(NewlinePolicy::CrOnly),
            "atascii" => Some(NewlinePolicy::AtasciiEol),
            other => {
                let rest = other.strip_prefix("rows-")?;
                let (number, parity) = rest.strip_suffix("-parity").map_or((rest, false), |n| (n, true));
                number.parse().ok().filter(|w| *w > 0).map(|width| NewlinePolicy::FixedRows { width, parity })
            }
        }
    }

    pub fn is_rows(self) -> bool {
        matches!(self, NewlinePolicy::FixedRows { .. })
    }

    pub fn parity(self) -> bool {
        matches!(self, NewlinePolicy::FixedRows { parity: true, .. })
    }
}

/// An optional way of reading the text on screen, kept apart from its character set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Display {
    Plain,
    AmigaConsole,
    /// PC ANSI art: ESC [ sequences shown as tokens, and a preview drawn as a DOS screen would show it.
    AnsiArt,
    Petscii,
    Teletext,
    Mode7,
}

impl Display {
    pub const ALL: [Display; 6] = [Display::Plain, Display::AmigaConsole, Display::AnsiArt, Display::Petscii, Display::Teletext, Display::Mode7];

    pub fn name(self) -> &'static str {
        match self {
            Display::Plain => "Plain text",
            Display::AmigaConsole => "Amiga console",
            Display::AnsiArt => "PC ANSI art",
            Display::Petscii => "PETSCII",
            Display::Teletext => "Teletext",
            Display::Mode7 => "BBC Mode 7",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Display::Plain => "plain",
            Display::AmigaConsole => "amiga",
            Display::AnsiArt => "ansi",
            Display::Petscii => "petscii",
            Display::Teletext => "teletext",
            Display::Mode7 => "mode7",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|display| display.key() == key)
    }
}

/// How sure the shown encoding is. Anything less than certain is offered as a suggestion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Certainty {
    /// A byte order mark says so.
    Bom,
    /// Bytes above 127 are all valid in this encoding.
    Valid,
    /// Only ASCII so far, which many encodings would read the same way.
    AsciiOnly,
    /// A guess from the bytes.
    Suggested,
    /// Remembered from an earlier explicit choice for this file.
    Remembered,
    /// Chosen by the person, or by a preset.
    Chosen,
}

impl Certainty {
    pub fn describe(self) -> &'static str {
        match self {
            Certainty::Bom => "Identified by the file's byte order mark.",
            Certainty::Valid => "Every byte is valid UTF-8.",
            Certainty::AsciiOnly => "Only ASCII so far: many encodings would read this the same way.",
            Certainty::Suggested => "A suggestion from the bytes, not a confirmed identification. Choose another under Reopen if it looks wrong.",
            Certainty::Remembered => "Your earlier choice for this file.",
            Certainty::Chosen => "Chosen by you or a preset.",
        }
    }
}

/// How a file's bytes become text, so that saving can reverse it.
#[derive(Clone, Debug)]
pub struct TextFormat {
    pub charset: &'static Charset,
    pub has_bom: bool,
    pub newline: NewlinePolicy,
    /// The table in force at the start of the stream, for sets with more than one.
    pub state: usize,
    pub display: Display,
    /// Add a line ending at the end when saving; off for existing files.
    pub ensure_final_newline: bool,
    /// What a new line uses when the file itself doesn't settle it.
    pub preset_eol: Eol,
    pub preset: Option<&'static str>,
    pub certainty: Certainty,
}

impl std::fmt::Debug for Charset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.id)
    }
}

impl Default for TextFormat {
    fn default() -> Self {
        Self {
            charset: charset::utf8(),
            has_bom: false,
            newline: NewlinePolicy::Standard,
            state: 0,
            display: Display::Plain,
            ensure_final_newline: false,
            preset_eol: Eol::CrLf,
            preset: None,
            certainty: Certainty::Chosen,
        }
    }
}

impl TextFormat {
    /// The status bar's words: `UTF-8 BOM` as in the original's picture, with `?` on a suggestion.
    pub fn encoding_name(&self) -> String {
        let name = match self.charset.family {
            Family::Unicode(_) => self.charset.name.clone(),
            Family::Multi(_) | Family::Bytes => self.charset.name.split(" (").next().unwrap_or(&self.charset.name).to_string(),
        };
        let bom = if self.has_bom { " BOM" } else { "" };
        let doubt = if self.certainty == Certainty::Suggested { "?" } else { "" };
        format!("{name}{bom}{doubt}")
    }

    pub fn tracker(&self) -> CellState {
        CellState::new(self.charset.model, self.state, self.newline.parity())
    }

    /// Text as typed or pasted, made ready for this file: each character stands for what it'll save as.
    pub fn prepare_input(&self, text: &str, state: usize) -> String {
        if self.charset.holds_characters() {
            return text.to_string();
        }
        text.chars().map(|c| if matches!(c, '\r' | '\n') { c } else { self.charset.for_text(c, state) }).collect()
    }

    /// Bytes as readable text for other programs, where each has a character to stand for it.
    pub fn readable_text(&self, text: &str) -> String {
        text.chars()
            .map(|c| match raw_byte(c) {
                Some(byte) => self.charset.entry(self.state, byte).and_then(|entry| entry.unicode).unwrap_or(c),
                None => c,
            })
            .collect()
    }
}

/// What a preset fills in. Each setting can still be changed on its own afterwards.
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub charset: &'static str,
    pub eol: Eol,
    pub newline: NewlinePolicy,
    pub display: Display,
    pub notes: &'static str,
}

const fn preset(id: &'static str, name: &'static str, charset: &'static str, eol: Eol, newline: NewlinePolicy, display: Display, notes: &'static str) -> Preset {
    Preset { id, name, charset, eol, newline, display, notes }
}

const ROWS: NewlinePolicy = NewlinePolicy::FixedRows { width: 40, parity: false };
const STD: NewlinePolicy = NewlinePolicy::Standard;

/// Defaults for new files or an explicit choice, not a promise about any existing file.
pub const PRESETS: [Preset; 22] = [
    preset("modern-windows", "Modern Windows", "utf-8", Eol::CrLf, STD, Display::Plain, "UTF-8 with no BOM, CRLF. UTF-8 with a BOM is under the encoding menu."),
    preset("legacy-windows", "Legacy Western Windows", "windows-1252", Eol::CrLf, STD, Display::Plain, "Other Windows code pages can be chosen under Reopen."),
    preset("dos-us", "DOS US", "cp437", Eol::CrLf, STD, Display::Plain, "A trailing Ctrl-Z is kept as a byte."),
    preset("dos-western", "DOS Western Europe", "cp850", Eol::CrLf, STD, Display::Plain, "Distinct from Windows-1252."),
    preset("modern-mac", "Modern macOS", "utf-8", Eol::Lf, STD, Display::Plain, "UTF-8 with no BOM, LF."),
    preset("classic-mac", "Classic Macintosh Western", "mac-roman", Eol::Cr, STD, Display::Plain, "Other Mac encodings can be chosen under Reopen."),
    preset("linux", "Linux / Unix", "utf-8", Eol::Lf, STD, Display::Plain, "Legacy encodings stay selectable."),
    preset("amiga-plain", "Amiga plain text", "iso-8859-1", Eol::Lf, STD, Display::Plain, "C1 control bytes are kept exactly."),
    preset("amiga-ansi", "Amiga ANSI", "amiga-text", Eol::Lf, STD, Display::AmigaConsole, "Amiga text encoding with the separate Amiga console reading. Not Windows-1252, and not PC ANSI art."),
    preset("pc-ansi", "PC ANSI art", "cp437", Eol::CrLf, STD, Display::AnsiArt, "CP437 with ANSI.SYS colour and cursor sequences, as BBS art was made. The preview draws it, using the SAUCE record if there is one."),
    preset("commodore", "Commodore PET / VIC-20 / C64", "petscii-c64", Eol::Cr, NewlinePolicy::CrOnly, Display::Petscii, "Choose the machine and starting character set in the Character table."),
    preset("atari-st", "Atari ST", "atari-st", Eol::CrLf, STD, Display::Plain, "Separate from Atari 8-bit."),
    preset("atari-8bit", "Atari 8-bit", "atascii", Eol::Atascii, NewlinePolicy::AtasciiEol, Display::Plain, "End of line is byte 0x9B. CR and LF bytes are ordinary data here."),
    preset("zx-spectrum", "ZX Spectrum text", "zx-spectrum", Eol::Cr, NewlinePolicy::CrOnly, Display::Plain, "Plain text. Tokenised BASIC programs, 48K, 128K and Next, are listed and made under Format, Tokenised BASIC."),
    preset("bbc-micro", "BBC Micro text", "bbc-micro", Eol::Cr, STD, Display::Plain, "Mode 7 is a separate display profile."),
    preset("teletext", "Teletext", "teletext-en", Eol::None, ROWS, Display::Teletext, "Raw rows of 40 cells with no line endings, English national set. Choose the national set in the Character table."),
    preset("ibm-mainframe", "IBM mainframe text (EBCDIC)", "ebcdic-037", Eol::Nl, STD, Display::Plain, "EBCDIC code page 037 with NL (0x15) line endings, as z/OS Unix text has. Other countries' code pages, and 1047, can be chosen under Reopen."),
    preset("ibm-records", "IBM mainframe records (EBCDIC, 80 columns)", "ebcdic-037", Eol::None, NewlinePolicy::FixedRows { width: 80, parity: false }, Display::Plain, "Fixed 80-byte records with nothing between them, as a data set sent from a mainframe in binary is."),
    preset("cpm", "CP/M text", "us-ascii", Eol::CrLf, STD, Display::Plain, "ASCII to begin with; Ctrl-Z and padding after it are kept. Machine sets can be chosen under Reopen."),
    preset("apple-ii", "Apple II text", "apple2", Eol::Cr, NewlinePolicy::CrOnly, Display::Plain, "High-bit ASCII as DOS 3.3 keeps text, with CR line endings. ProDOS text is usually plain ASCII."),
    preset("msx", "MSX text", "msx", Eol::CrLf, STD, Display::Plain, "International MSX. Regional sets can be chosen under Reopen. Never treated as CP437."),
    preset("ascii", "Generic ASCII", "us-ascii", Eol::CrLf, STD, Display::Plain, "Strict 7-bit: bytes above 0x7F are flagged and kept. Line ending is yours to choose."),
];

pub fn preset_by_id(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|preset| preset.id == id)
}

/// Retro formats whose file ending says what they are, for a file opened with nothing chosen for it.
/// It's a suggestion like any other, shown with a `?`, and Reopen can still change it.
pub fn preset_for_extension(path: &Path) -> Option<&'static Preset> {
    let extension = path.extension()?.to_str()?.to_lowercase();
    let id = match extension.as_str() {
        "ans" | "ice" | "diz" | "nfo" | "xb" | "pcb" | "avt" => "pc-ansi",
        "seq" => "commodore",
        "ep1" => "teletext",
        "ebc" | "ebcdic" => "ibm-mainframe",
        _ => return None,
    };
    preset_by_id(id)
}

impl Preset {
    pub fn format(&'static self) -> TextFormat {
        TextFormat {
            charset: charset::known(self.charset),
            has_bom: false,
            newline: self.newline,
            state: 0,
            display: self.display,
            ensure_final_newline: false,
            preset_eol: self.eol,
            preset: Some(self.id),
            certainty: Certainty::Chosen,
        }
    }
}

// ---------- Detection ----------

pub struct Detection {
    pub charset: &'static Charset,
    pub has_bom: bool,
    pub certainty: Certainty,
}

/// A byte order mark is strong evidence; everything else is only checked or suggested.
/// PETSCII, ATASCII, Teletext and the like are never guessed at from the bytes.
pub fn detect(data: &[u8]) -> Detection {
    let sure = |id: &str| Detection { charset: charset::known(id), has_bom: true, certainty: Certainty::Bom };
    if data.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) && data.len().is_multiple_of(4) {
        return sure("utf-32le");
    }
    if data.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        return sure("utf-32be");
    }
    if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return sure("utf-8");
    }
    if data.starts_with(&[0xFF, 0xFE]) {
        return sure("utf-16le");
    }
    if data.starts_with(&[0xFE, 0xFF]) {
        return sure("utf-16be");
    }
    let plain = |id: &str, certainty| Detection { charset: charset::known(id), has_bom: false, certainty };
    if data.is_ascii() && !data.contains(&0) {
        return plain("utf-8", Certainty::AsciiOnly);
    }
    if !data.contains(&0) && std::str::from_utf8(data).is_ok() {
        return plain("utf-8", Certainty::Valid);
    }
    if let Some(id) = looks_like_utf16(data) {
        return plain(id, Certainty::Suggested);
    }
    if !data.contains(&0) && mostly_utf8(data) {
        return plain("utf-8", Certainty::Suggested);
    }
    plain(suggest_legacy(data), Certainty::Suggested)
}

/// UTF-8 with a few bad bytes in it, as opposed to a set whose accents only look like bad UTF-8.
fn mostly_utf8(data: &[u8]) -> bool {
    let (mut good, mut bad) = (0usize, 0usize);
    let mut rest = &data[..data.len().min(65536)];
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                good += valid.chars().filter(|c| !c.is_ascii()).count();
                break;
            }
            Err(problem) => {
                let (valid, tail) = rest.split_at(problem.valid_up_to());
                good += std::str::from_utf8(valid).map_or(0, |v| v.chars().filter(|c| !c.is_ascii()).count());
                let Some(skip) = problem.error_len() else {
                    break;
                };
                bad += skip;
                rest = &tail[skip..];
            }
        }
    }
    good > 0 && good >= bad
}

/// Text in UTF-16 with no mark has a zero in about every other byte, nearly all on one side.
fn looks_like_utf16(data: &[u8]) -> Option<&'static str> {
    if data.len() < 8 || !data.len().is_multiple_of(2) {
        return None;
    }
    let sample = &data[..data.len().min(65536)];
    let pairs = sample.len() / 2;
    let low_zero = sample.as_chunks::<2>().0.iter().filter(|p| p[1] == 0).count();
    let high_zero = sample.as_chunks::<2>().0.iter().filter(|p| p[0] == 0).count();
    if low_zero * 10 >= pairs * 7 && high_zero * 20 < pairs {
        Some("utf-16le")
    } else if high_zero * 10 >= pairs * 7 && low_zero * 20 < pairs {
        Some("utf-16be")
    } else {
        None
    }
}

/// Box-drawing bytes are a mark of DOS text, and EBCDIC's letters and space a mark of mainframe text;
/// otherwise the commonest Western set is only a suggestion.
fn suggest_legacy(data: &[u8]) -> &'static str {
    if looks_like_ebcdic(data) {
        return "ebcdic-037";
    }
    let high: Vec<u8> = data.iter().take(65536).copied().filter(|b| *b >= 0x80).collect();
    let boxes = high.iter().filter(|b| (0xB0..=0xDF).contains(*b)).count();
    if high.len() >= 8 && boxes * 2 > high.len() { "cp437" } else { "windows-1252" }
}

/// EBCDIC text is mostly its space (0x40), letters and digits, all of which are above 0x80 or odd
/// punctuation in ASCII, so ASCII text never looks like it.
pub fn looks_like_ebcdic(data: &[u8]) -> bool {
    let sample = &data[..data.len().min(65536)];
    if sample.len() < 64 {
        return false;
    }
    let letter = |b: u8| matches!(b, 0x81..=0x89 | 0x91..=0x99 | 0xA2..=0xA9 | 0xC1..=0xC9 | 0xD1..=0xD9 | 0xE2..=0xE9 | 0xF0..=0xF9);
    let spaces = sample.iter().filter(|b| **b == 0x40).count();
    let text = sample.iter().filter(|b| **b == 0x40 || letter(**b) || matches!(**b, 0x4B | 0x5B | 0x5C | 0x5D | 0x5E | 0x60 | 0x61 | 0x6B | 0x7D | 0x7E | 0x15 | 0x25 | 0x0D)).count();
    let ascii_letters = sample.iter().filter(|b| b.is_ascii_alphabetic()).count();
    spaces * 20 >= sample.len() && text * 10 >= sample.len() * 8 && ascii_letters * 10 < sample.len()
}

/// Mainframe data sent in binary is fixed records, so a file with no EBCDIC line endings at all whose length
/// is a whole number of 80-byte records is taken to be card images.
pub fn ebcdic_rows(data: &[u8]) -> Option<NewlinePolicy> {
    let unbroken = !data.iter().any(|b| matches!(*b, 0x15 | 0x25 | 0x0D));
    (unbroken && !data.is_empty() && data.len().is_multiple_of(80)).then_some(NewlinePolicy::FixedRows { width: 80, parity: false })
}

/// Whether a file should open in hex because it holds NUL bytes and nothing says it's text.
pub fn looks_binary(data: &[u8]) -> bool {
    let d = detect(data);
    if d.has_bom || (d.charset.holds_characters() && d.certainty == Certainty::Suggested) {
        return false;
    }
    data.iter().take(65536).any(|b| *b == 0)
}

// ---------- Decoding ----------

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct EolCounts {
    pub crlf: usize,
    pub lf: usize,
    pub cr: usize,
    pub atascii: usize,
    pub nl: usize,
}

impl EolCounts {
    fn add(&mut self, eol: Eol, by: usize) {
        match eol {
            Eol::CrLf => self.crlf += by,
            Eol::Lf => self.lf += by,
            Eol::Cr => self.cr += by,
            Eol::Atascii => self.atascii += by,
            Eol::Nl => self.nl += by,
            Eol::None => {}
        }
    }

    pub fn of(eols: impl Iterator<Item = (Eol, usize)>) -> Self {
        let mut counts = Self::default();
        for (eol, by) in eols {
            counts.add(eol, by);
        }
        counts
    }

    pub fn kinds(&self) -> usize {
        [self.crlf, self.lf, self.cr, self.atascii, self.nl].iter().filter(|n| **n > 0).count()
    }

    pub fn is_mixed(&self) -> bool {
        self.kinds() > 1
    }

    /// The commonest ending; a tie or no endings at all fall back to what's given.
    pub fn predominant(&self, fallback: Eol) -> Eol {
        let counts = [(Eol::CrLf, self.crlf), (Eol::Lf, self.lf), (Eol::Cr, self.cr), (Eol::Atascii, self.atascii), (Eol::Nl, self.nl)];
        let most = counts.iter().map(|c| c.1).max().unwrap_or(0);
        if most == 0 {
            return fallback;
        }
        let top: Vec<Eol> = counts.iter().filter(|c| c.1 == most).map(|c| c.0).collect();
        if top.len() == 1 { top[0] } else { fallback }
    }

    pub fn describe(&self) -> String {
        let parts: Vec<String> = [("CRLF", self.crlf), ("LF", self.lf), ("CR", self.cr), ("ATASCII EOL", self.atascii), ("NL", self.nl)]
            .iter()
            .filter(|c| c.1 > 0)
            .map(|c| format!("{} {}", c.1, c.0))
            .collect();
        if parts.is_empty() { "No line endings".to_string() } else { parts.join(", ") }
    }
}

/// The lines of a file, each with its ending. Lines whose ending isn't `base` are listed in `exceptions`.
pub struct Decoded {
    pub lines: Vec<String>,
    pub base: Eol,
    pub exceptions: BTreeMap<usize, Eol>,
    pub counts: EolCounts,
}

/// Valid UTF-8 as itself and anything else as exact bytes. Valid text that happens to be in the range
/// used for exact bytes is kept as its separate bytes too, so the two can't be confused.
pub fn decode_utf8_lossless(data: &[u8], out: &mut String) {
    let mut rest = data;
    loop {
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                push_valid(valid, out);
                return;
            }
            Err(problem) => {
                let (good, bad) = rest.split_at(problem.valid_up_to());
                push_valid(std::str::from_utf8(good).unwrap_or(""), out);
                let skip = problem.error_len().unwrap_or(bad.len());
                out.extend(bad[..skip].iter().map(|b| raw_char(*b)));
                rest = &bad[skip..];
            }
        }
    }
}

fn push_valid(text: &str, out: &mut String) {
    if !text.as_bytes().contains(&0xF4) {
        out.push_str(text);
        return;
    }
    for c in text.chars() {
        if raw_byte(c).is_some() {
            out.extend(c.encode_utf8(&mut [0; 4]).bytes().map(raw_char));
        } else {
            out.push(c);
        }
    }
}

fn decode_utf16_lossless(body: &[u8], big_endian: bool) -> String {
    let unit = |pair: &[u8]| if big_endian { u16::from_be_bytes([pair[0], pair[1]]) } else { u16::from_le_bytes([pair[0], pair[1]]) };
    let mut out = String::with_capacity(body.len() / 2);
    let pairs: Vec<&[u8]> = body.as_chunks::<2>().0.iter().map(<[u8; 2]>::as_slice).collect();
    let mut i = 0;
    while i < pairs.len() {
        let first = unit(pairs[i]);
        let joined = if (0xD800..0xDC00).contains(&first) && i + 1 < pairs.len() && (0xDC00..0xE000).contains(&unit(pairs[i + 1])) {
            let second = unit(pairs[i + 1]);
            char::from_u32(0x10000 + ((u32::from(first) - 0xD800) << 10) + (u32::from(second) - 0xDC00)).map(|c| (c, 2))
        } else {
            char::from_u32(u32::from(first)).map(|c| (c, 1))
        };
        match joined {
            Some((c, used)) if raw_byte(c).is_none() => {
                out.push(c);
                i += used;
            }
            Some((_, used)) => {
                for pair in &pairs[i..i + used] {
                    out.extend(pair.iter().map(|b| raw_char(*b)));
                }
                i += used;
            }
            None => {
                out.extend(pairs[i].iter().map(|b| raw_char(*b)));
                i += 1;
            }
        }
    }
    if body.len() % 2 == 1 {
        out.push(raw_char(body[body.len() - 1]));
    }
    out
}

fn decode_utf32_lossless(body: &[u8], big_endian: bool) -> String {
    let mut out = String::with_capacity(body.len() / 4);
    let (quads, tail) = body.as_chunks::<4>();
    for quad in quads {
        let value = if big_endian { u32::from_be_bytes(*quad) } else { u32::from_le_bytes(*quad) };
        match char::from_u32(value).filter(|c| raw_byte(*c).is_none()) {
            Some(c) => out.push(c),
            None => out.extend(quad.iter().map(|b| raw_char(*b))),
        }
    }
    out.extend(tail.iter().map(|b| raw_char(*b)));
    out
}

/// Whether some bytes read as one character that's written back as exactly those bytes.
fn one_exact_char(encoding: &'static encoding_rs::Encoding, bytes: &[u8]) -> Option<char> {
    let text = encoding.decode_without_bom_handling_and_without_replacement(bytes)?;
    let mut chars = text.chars();
    let c = chars.next().filter(|c| raw_byte(*c).is_none())?;
    if chars.next().is_some() {
        return None;
    }
    let (back, _, unmappable) = encoding.encode(&text);
    (!unmappable && *back == *bytes).then_some(c)
}

/// UTF-7's base64 letters.
const UTF7_BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Characters UTF-7 writes as themselves. Python's encoder writes all printable ASCII but + ~ and \ so; glibc's
/// (`strict`) only RFC 2152's set D, space, tab and line ends, putting the optional characters in base64.
fn utf7_direct(c: char, strict: bool) -> bool {
    if strict {
        return c.is_ascii_alphanumeric() || matches!(c, '\'' | '(' | ')' | ',' | '-' | '.' | '/' | ':' | '?' | ' ' | '\t' | '\r' | '\n');
    }
    matches!(c, '\t' | '\r' | '\n') || (matches!(c, ' '..='~') && !matches!(c, '+' | '~' | '\\'))
}

/// Writes text as UTF-7 (RFC 2152), making the choices Python's encoder makes: a base64 run is closed with `-`
/// only before a base64 letter or `-`, or at the end when `close_at_end` says the text ends there.
pub fn utf7_encode(text: &str, close_at_end: bool, strict: bool, out: &mut Vec<u8>) {
    let (mut in_run, mut bits, mut count) = (false, 0u32, 0u32);
    let flush = |out: &mut Vec<u8>, bits: &mut u32, count: &mut u32| {
        if *count > 0 {
            out.push(UTF7_BASE64[((*bits << (6 - *count)) & 0x3F) as usize]);
            (*bits, *count) = (0, 0);
        }
    };
    for c in text.chars() {
        if utf7_direct(c, strict) {
            if in_run {
                flush(out, &mut bits, &mut count);
                if UTF7_BASE64.contains(&(c as u8)) || c == '-' {
                    out.push(b'-');
                }
                in_run = false;
            }
            out.push(c as u8);
        } else if c == '+' && !in_run {
            out.extend_from_slice(b"+-");
        } else {
            if !in_run {
                out.push(b'+');
                in_run = true;
            }
            for unit in c.encode_utf16(&mut [0; 2]) {
                bits = (bits << 16) | u32::from(*unit);
                count += 16;
                while count >= 6 {
                    count -= 6;
                    out.push(UTF7_BASE64[((bits >> count) & 0x3F) as usize]);
                }
                bits &= (1 << count) - 1;
            }
        }
    }
    if in_run {
        flush(out, &mut bits, &mut count);
        if close_at_end {
            out.push(b'-');
        }
    }
}

/// Reads UTF-7, strictly: None for anything RFC 2152 doesn't allow, such as bytes above 127 or loose bits.
pub fn utf7_decode(bytes: &[u8]) -> Option<String> {
    let mut out = String::new();
    let mut units: Vec<u16> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        i += 1;
        if byte >= 0x80 {
            return None;
        }
        if byte != b'+' {
            out.push(byte as char);
            continue;
        }
        if bytes.get(i) == Some(&b'-') {
            out.push('+');
            i += 1;
            continue;
        }
        let (mut bits, mut count) = (0u32, 0u32);
        while let Some(value) = bytes.get(i).and_then(|b| UTF7_BASE64.iter().position(|x| x == b)) {
            bits = (bits << 6) | value as u32;
            count += 6;
            if count >= 16 {
                count -= 16;
                units.push((bits >> count) as u16);
                bits &= (1 << count) - 1;
            }
            i += 1;
        }
        if count >= 6 || bits != 0 {
            return None;
        }
        out.push_str(&String::from_utf16(&units).ok()?);
        units.clear();
        if bytes.get(i) == Some(&b'-') {
            i += 1;
        }
    }
    Some(out)
}

/// Multi-byte text, a character at a time: each is kept as a character only if writing it gives back the same
/// bytes, so a sequence the set doesn't read, or reads two ways, stays exactly as it was.
pub fn decode_multi_lossless(body: &[u8], form: MultiForm, out: &mut String) {
    let encoding = form.encoding();
    if matches!(form, MultiForm::Utf7 | MultiForm::Utf7Strict) {
        // UTF-7 is read a line at a time; a line is text only if writing it back gives the same bytes.
        let strict = form == MultiForm::Utf7Strict;
        let mut start = 0;
        for end in (0..=body.len()).filter(|&i| i == body.len() || matches!(body[i], b'\r' | b'\n')) {
            let line = &body[start..end];
            let back = |text: &str| {
                let mut bytes = Vec::new();
                utf7_encode(text, end == body.len(), strict, &mut bytes);
                bytes
            };
            match utf7_decode(line) {
                Some(text) if !text.chars().any(|c| raw_byte(c).is_some() || matches!(c, '\r' | '\n')) && back(&text) == line => out.push_str(&text),
                _ => out.extend(line.iter().map(|b| raw_char(*b))),
            }
            if let Some(ending) = body.get(end) {
                out.push(*ending as char);
            }
            start = end + 1;
        }
        return;
    }
    if form.is_stateful() {
        // ISO-2022-JP returns to ASCII at each line's end, so each line between CR and LF bytes is read on its own.
        let mut start = 0;
        for end in (0..=body.len()).filter(|&i| i == body.len() || matches!(body[i], b'\r' | b'\n')) {
            let line = &body[start..end];
            match encoding.decode_without_bom_handling_and_without_replacement(line) {
                Some(text) if !text.chars().any(|c| raw_byte(c).is_some()) && *encoding.encode(&text).0 == *line => out.push_str(&text),
                _ => out.extend(line.iter().map(|b| raw_char(*b))),
            }
            if let Some(ending) = body.get(end) {
                out.push(*ending as char);
            }
            start = end + 1;
        }
        return;
    }
    let mut i = 0;
    while i < body.len() {
        let byte = body[i];
        if byte < 0x80 {
            out.push(byte as char);
            i += 1;
            continue;
        }
        match (1..=4).find_map(|length| body.get(i..i + length).and_then(|bytes| one_exact_char(encoding, bytes)).map(|c| (c, length))) {
            Some((c, length)) => {
                out.push(c);
                i += length;
            }
            None => {
                out.push(raw_char(byte));
                i += 1;
            }
        }
    }
}

/// One line's bytes, already split from its ending, for files read a block at a time.
pub fn decode_line(bytes: &[u8], charset: &Charset) -> String {
    match charset.family {
        Family::Unicode(_) => {
            let mut out = String::with_capacity(bytes.len());
            decode_utf8_lossless(bytes, &mut out);
            out
        }
        Family::Multi(form) => {
            let mut out = String::with_capacity(bytes.len());
            decode_multi_lossless(bytes, form, &mut out);
            out
        }
        Family::Bytes => bytes.iter().map(|b| charset.decoded[usize::from(*b)]).collect(),
    }
}

/// Turns a file's bytes into lines, following the format's settings exactly. Works out whether the
/// mark is really there, so a file that has one keeps it and one that doesn't never gains it.
pub fn decode(data: &[u8], format: &mut TextFormat) -> Decoded {
    let bom = format.charset.bom().filter(|mark| data.starts_with(mark));
    format.has_bom = bom.is_some();
    let body = &data[bom.map_or(0, <[u8]>::len)..];
    let mut builder = LineBuilder::default();
    match format.charset.family {
        Family::Unicode(form) => {
            let text = match form {
                UnicodeForm::Utf8 => {
                    let mut out = String::with_capacity(body.len());
                    decode_utf8_lossless(body, &mut out);
                    out
                }
                UnicodeForm::Utf16Le => decode_utf16_lossless(body, false),
                UnicodeForm::Utf16Be => decode_utf16_lossless(body, true),
                UnicodeForm::Utf32Le => decode_utf32_lossless(body, false),
                UnicodeForm::Utf32Be => decode_utf32_lossless(body, true),
            };
            split_characters(&text, format.newline, &mut builder);
        }
        Family::Multi(form) => {
            let mut text = String::with_capacity(body.len());
            decode_multi_lossless(body, form, &mut text);
            split_characters(&text, format.newline, &mut builder);
        }
        Family::Bytes => split_bytes(body, format.charset, format.newline, &mut builder),
    }
    builder.finish(format)
}

#[derive(Default)]
struct LineBuilder {
    lines: Vec<String>,
    current: String,
    endings: Vec<Eol>,
}

impl LineBuilder {
    fn end_line(&mut self, eol: Eol) {
        self.lines.push(std::mem::take(&mut self.current));
        self.endings.push(eol);
    }

    fn finish(mut self, format: &TextFormat) -> Decoded {
        let is_rows = format.newline.is_rows() && !format.charset.holds_characters();
        // Rows have no ending, so a whole last row isn't followed by an empty one.
        if !is_rows || !self.current.is_empty() || self.lines.is_empty() {
            self.lines.push(std::mem::take(&mut self.current));
        } else {
            self.endings.pop();
        }
        let counts = EolCounts::of(self.endings.iter().map(|eol| (*eol, 1)));
        let base = if is_rows { Eol::None } else { counts.predominant(format.preset_eol) };
        let exceptions = self.endings.iter().enumerate().filter(|(_, eol)| **eol != base).map(|(line, eol)| (line, *eol)).collect();
        Decoded { lines: self.lines, base, exceptions, counts }
    }
}

fn split_characters(text: &str, policy: NewlinePolicy, out: &mut LineBuilder) {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, policy) {
            ('\r', NewlinePolicy::Standard) => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                    out.end_line(Eol::CrLf);
                } else {
                    out.end_line(Eol::Cr);
                }
            }
            ('\n', NewlinePolicy::Standard) => out.end_line(Eol::Lf),
            ('\r', NewlinePolicy::CrOnly) => out.end_line(Eol::Cr),
            // In Unicode text no other code is ever a line ending, so NEL and the like stay in the line.
            _ => out.current.push(c),
        }
    }
}

fn split_bytes(body: &[u8], charset: &Charset, policy: NewlinePolicy, out: &mut LineBuilder) {
    if let NewlinePolicy::FixedRows { width, .. } = policy {
        for row in body.chunks(width.max(1)) {
            out.current.extend(row.iter().map(|b| charset.decoded[usize::from(*b)]));
            if row.len() == width.max(1) {
                out.end_line(Eol::None);
            }
        }
        return;
    }
    // EBCDIC has its own line feed, and a new line control as well.
    let (lf, nl) = if charset.ebcdic { (0x25, Some(0x15)) } else { (0x0A, None) };
    let mut i = 0;
    while i < body.len() {
        let byte = body[i];
        i += 1;
        match (byte, policy) {
            (b, NewlinePolicy::Standard) if Some(b) == nl => out.end_line(Eol::Nl),
            (0x0D, NewlinePolicy::Standard) => {
                if body.get(i) == Some(&lf) {
                    i += 1;
                    out.end_line(Eol::CrLf);
                } else {
                    out.end_line(Eol::Cr);
                }
            }
            (b, NewlinePolicy::Standard) if b == lf => out.end_line(Eol::Lf),
            (0x0D, NewlinePolicy::CrOnly) => out.end_line(Eol::Cr),
            (0x9B, NewlinePolicy::AtasciiEol) => out.end_line(Eol::Atascii),
            _ => out.current.push(charset.decoded[usize::from(byte)]),
        }
    }
}

// ---------- Encoding ----------

/// A character that can't be saved in the encoding, and where it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Problem {
    pub line: usize,
    /// Zero-based character position in the line.
    pub column: usize,
    pub ch: char,
}

/// Everything that stands in the way of an encoding, listed before anything is written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Problems {
    pub listed: Vec<Problem>,
    pub total: usize,
}

impl Problems {
    pub fn add(&mut self, problem: Problem) {
        self.total += 1;
        if self.listed.len() < MAX_LISTED_PROBLEMS {
            self.listed.push(problem);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }
}

/// What to do with a character that can't be stored. Anything but Refuse is an explicit export choice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lossy {
    Refuse,
    Replace,
    Transliterate,
    Omit,
}

fn write_unicode(form: UnicodeForm, c: char, out: &mut Vec<u8>) {
    match form {
        UnicodeForm::Utf8 => out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes()),
        UnicodeForm::Utf16Le => {
            for unit in c.encode_utf16(&mut [0; 2]) {
                out.extend_from_slice(&unit.to_le_bytes());
            }
        }
        UnicodeForm::Utf16Be => {
            for unit in c.encode_utf16(&mut [0; 2]) {
                out.extend_from_slice(&unit.to_be_bytes());
            }
        }
        UnicodeForm::Utf32Le => out.extend_from_slice(&(c as u32).to_le_bytes()),
        UnicodeForm::Utf32Be => out.extend_from_slice(&(c as u32).to_be_bytes()),
    }
}

/// The bytes of a line ending in this encoding.
pub fn eol_bytes(eol: Eol, charset: &Charset) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    if charset.ebcdic {
        return match eol {
            Eol::CrLf => vec![0x0D, 0x25],
            Eol::Lf => vec![0x25],
            Eol::Cr => vec![0x0D],
            Eol::Nl => vec![0x15],
            Eol::Atascii => vec![0x9B],
            Eol::None => out,
        };
    }
    let chars: &[char] = match eol {
        Eol::CrLf => &['\r', '\n'],
        Eol::Lf => &['\n'],
        Eol::Cr => &['\r'],
        // Outside EBCDIC, the nearest thing is Unicode's and ISO 6429's next line, NEL.
        Eol::Nl => &['\u{85}'],
        Eol::Atascii => {
            out.push(0x9B);
            return out;
        }
        Eol::None => return out,
    };
    match charset.family {
        Family::Unicode(form) => {
            for c in chars {
                write_unicode(form, *c, &mut out);
            }
        }
        Family::Multi(_) | Family::Bytes => out.extend(chars.iter().map(|c| *c as u32 as u8)),
    }
    out
}

/// Writes what's left of a stateful encoder, such as ISO-2022-JP's return to ASCII.
fn finish_encoder(encoder: &mut encoding_rs::Encoder, out: &mut Vec<u8>) {
    let mut buffer = [0u8; 16];
    let (_, _, written) = encoder.encode_from_utf8_without_replacement("", &mut buffer, true);
    out.extend_from_slice(&buffer[..written]);
}

/// One character in a multi-byte set, or None if the set hasn't got it.
fn multi_char_bytes(c: char, form: MultiForm) -> Option<Vec<u8>> {
    if matches!(form, MultiForm::Utf7 | MultiForm::Utf7Strict) {
        let mut out = Vec::new();
        utf7_encode(c.encode_utf8(&mut [0; 4]), true, form == MultiForm::Utf7Strict, &mut out);
        return Some(out);
    }
    let mut encoder = form.encoding().new_encoder();
    let mut buffer = [0u8; 16];
    let (result, _, written) = encoder.encode_from_utf8_without_replacement(c.encode_utf8(&mut [0; 4]), &mut buffer, false);
    if result != encoding_rs::EncoderResult::InputEmpty {
        return None;
    }
    let mut out = buffer[..written].to_vec();
    finish_encoder(&mut encoder, &mut out);
    Some(out)
}

/// The bytes one character takes in this encoding, or None if it can't be stored.
pub fn char_bytes(c: char, charset: &Charset, state: usize) -> Option<Vec<u8>> {
    if let Some(byte) = raw_byte(c) {
        return Some(vec![byte]);
    }
    match charset.family {
        Family::Unicode(form) => {
            let mut out = Vec::with_capacity(4);
            write_unicode(form, c, &mut out);
            Some(out)
        }
        Family::Multi(form) => multi_char_bytes(c, form),
        Family::Bytes => charset.table(state).and_then(|table| table.byte_of(c)).map(|byte| vec![byte]),
    }
}

/// What a character becomes when it has to fit a smaller set, for an export that asks for it.
/// This is the only place text is decomposed, and only by that explicit choice.
pub fn transliterate(c: char) -> String {
    let punctuation = match c {
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => Some("'"),
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => Some("\""),
        '\u{2013}' | '\u{2014}' | '\u{2015}' | '\u{2212}' => Some("-"),
        '\u{2026}' => Some("..."),
        '\u{A0}' | '\u{2002}' | '\u{2003}' | '\u{2009}' => Some(" "),
        '\u{20AC}' => Some("EUR"),
        '\u{A3}' => Some("GBP"),
        '\u{A9}' => Some("(c)"),
        '\u{AE}' => Some("(R)"),
        '\u{2122}' => Some("(TM)"),
        '\u{2022}' => Some("*"),
        '\u{DF}' => Some("ss"),
        '\u{C6}' => Some("AE"),
        '\u{E6}' => Some("ae"),
        '\u{152}' => Some("OE"),
        '\u{153}' => Some("oe"),
        '\u{D8}' => Some("O"),
        '\u{F8}' => Some("o"),
        _ => None,
    };
    if let Some(text) = punctuation {
        return text.to_string();
    }
    c.to_string().nfd().filter(|d| !matches!(*d as u32, 0x300..=0x36F)).collect()
}

/// Writes one line's characters. Exact bytes go out as they are; the rest go through the set's own mapping.
#[allow(clippy::too_many_arguments)]
fn encode_line(line: &str, number: usize, last: bool, format: &TextFormat, tracker: &mut CellState, lossy: Lossy, problems: &mut Problems, out: &mut Vec<u8>) {
    match format.charset.family {
        Family::Unicode(form) => {
            if form == UnicodeForm::Utf8 && !line.as_bytes().contains(&0xF4) {
                out.extend_from_slice(line.as_bytes());
                return;
            }
            for c in line.chars() {
                match raw_byte(c) {
                    Some(byte) => out.push(byte),
                    None => write_unicode(form, c, out),
                }
            }
        }
        Family::Multi(form @ (MultiForm::Utf7 | MultiForm::Utf7Strict)) => {
            // Text between exact bytes is written as UTF-7, its base64 closed before a byte and at the file's end.
            let strict = form == MultiForm::Utf7Strict;
            let mut run = String::new();
            for c in line.chars() {
                match raw_byte(c) {
                    Some(byte) => {
                        utf7_encode(&std::mem::take(&mut run), true, strict, out);
                        out.push(byte);
                    }
                    None => run.push(c),
                }
            }
            utf7_encode(&run, last, strict, out);
        }
        Family::Multi(form) => {
            let encoding = form.encoding();
            let mut encoder = encoding.new_encoder();
            let mut buffer = [0u8; 16];
            for (column, c) in line.chars().enumerate() {
                if let Some(byte) = raw_byte(c) {
                    // An exact byte goes out as it is, with the encoder back in its starting state either side.
                    finish_encoder(&mut encoder, out);
                    encoder = encoding.new_encoder();
                    out.push(byte);
                    continue;
                }
                let (result, _, written) = encoder.encode_from_utf8_without_replacement(c.encode_utf8(&mut [0; 4]), &mut buffer, false);
                out.extend_from_slice(&buffer[..written]);
                if result != encoding_rs::EncoderResult::InputEmpty {
                    finish_encoder(&mut encoder, out);
                    encoder = encoding.new_encoder();
                    fit_or_report(c, number, column, format, lossy, problems, out);
                }
            }
            finish_encoder(&mut encoder, out);
        }
        Family::Bytes => {
            tracker.next_row();
            for (column, c) in line.chars().enumerate() {
                let byte = raw_byte(c).or_else(|| format.charset.table(tracker.table.min(1)).and_then(|table| table.byte_of(c)));
                match byte {
                    Some(byte) => {
                        out.push(byte);
                        tracker.advance(byte);
                    }
                    None => fit_or_report(c, number, column, format, lossy, problems, out),
                }
            }
        }
    }
}

fn fit_or_report(c: char, line: usize, column: usize, format: &TextFormat, lossy: Lossy, problems: &mut Problems, out: &mut Vec<u8>) {
    let table = format.charset.table(format.state);
    // A set that holds characters writes ASCII as itself; a table set looks it up.
    let byte_for = |ch: char| if format.charset.holds_characters() { ch.is_ascii().then_some(ch as u8) } else { table.and_then(|t| t.byte_of(ch)) };
    let question = byte_for('?');
    match lossy {
        Lossy::Refuse => problems.add(Problem { line, column, ch: c }),
        Lossy::Omit => {}
        Lossy::Replace => out.extend(question),
        Lossy::Transliterate => {
            for simple in transliterate(c).chars() {
                out.extend(byte_for(simple).or(question));
            }
        }
    }
}

/// Encodes lines for saving. A character the encoding can't hold is reported with its place, rather
/// than being quietly replaced. Lines keep the endings given for them; the last has none.
pub fn encode_with<S: AsRef<str>>(lines: &[S], eol_of: &dyn Fn(usize) -> Eol, format: &TextFormat, lossy: Lossy) -> Result<Vec<u8>, Problems> {
    let mut out = Vec::with_capacity(lines.iter().map(|l| l.as_ref().len() + 2).sum::<usize>() + 4);
    if format.has_bom
        && let Some(mark) = format.charset.bom()
    {
        out.extend_from_slice(mark);
    }
    let mut problems = Problems::default();
    let mut tracker = format.tracker();
    let mut endings: Vec<(Eol, Vec<u8>)> = Vec::with_capacity(4);
    for (i, line) in lines.iter().enumerate() {
        encode_line(line.as_ref(), i, i + 1 == lines.len(), format, &mut tracker, lossy, &mut problems, &mut out);
        if i + 1 < lines.len() {
            let eol = eol_of(i);
            if !endings.iter().any(|known| known.0 == eol) {
                endings.push((eol, eol_bytes(eol, format.charset)));
            }
            if let Some(known) = endings.iter().find(|known| known.0 == eol) {
                out.extend_from_slice(&known.1);
            }
        }
    }
    if problems.is_empty() { Ok(out) } else { Err(problems) }
}

pub fn encode<S: AsRef<str>>(lines: &[S], eol_of: &dyn Fn(usize) -> Eol, format: &TextFormat) -> Result<Vec<u8>, Problems> {
    encode_with(lines, eol_of, format, Lossy::Refuse)
}

/// Rewrites text for another encoding, keeping what it says. An exact byte becomes the character its
/// current set says it is; one with no character, or a character the new set hasn't got, is a problem.
pub fn convert_lines<S: AsRef<str>>(lines: &[S], from: &TextFormat, to: &TextFormat) -> Result<Vec<String>, Problems> {
    let mut problems = Problems::default();
    let mut tracker = from.tracker();
    let mut to_tracker = to.tracker();
    let mut out = Vec::with_capacity(lines.len());
    for (number, line) in lines.iter().enumerate() {
        tracker.next_row();
        to_tracker.next_row();
        let mut converted = String::with_capacity(line.as_ref().len());
        for (column, c) in line.as_ref().chars().enumerate() {
            let meant = match raw_byte(c) {
                Some(byte) => {
                    let entry = from.charset.entry(tracker.table_for(from.charset, byte), tracker.lookup(byte));
                    tracker.advance(byte);
                    entry.and_then(|entry| entry.unicode)
                }
                None => Some(c),
            };
            let placed = meant.map(|c| to.charset.for_text(c, to_tracker.table.min(to.charset.tables.len().saturating_sub(1))));
            match placed.filter(|placed| fits(*placed, to.charset)) {
                Some(placed) => {
                    if let Some(byte) = raw_byte(placed) {
                        to_tracker.advance(byte);
                    }
                    converted.push(placed);
                }
                None => problems.add(Problem { line: number, column, ch: c }),
            }
        }
        out.push(converted);
    }
    if problems.is_empty() { Ok(out) } else { Err(problems) }
}

/// The bytes of a copy in another encoding. Line endings stay as they are. With Refuse, anything that
/// won't fit is reported; the other choices are for an export that has asked to lose them.
pub fn export<S: AsRef<str>>(lines: &[S], eol_of: &dyn Fn(usize) -> Eol, from: &TextFormat, to: &TextFormat, lossy: Lossy) -> Result<Vec<u8>, Problems> {
    match convert_lines(lines, from, to) {
        Ok(converted) => encode(&converted, eol_of, to),
        Err(problems) if lossy == Lossy::Refuse => Err(problems),
        Err(_) => {
            let readable: Vec<String> = lines
                .iter()
                .map(|line| {
                    let text = from.readable_text(line.as_ref());
                    // A byte with no character behind it can't be carried over.
                    text.chars().filter_map(|c| if raw_byte(c).is_none() { Some(c) } else if lossy == Lossy::Omit { None } else { Some('?') }).collect()
                })
                .collect();
            encode_with(&readable, eol_of, to, lossy)
        }
    }
}

/// Whether a character in the text will save in this set as itself or as its byte.
pub fn fits(c: char, set: &Charset) -> bool {
    match set.family {
        Family::Unicode(_) => true,
        Family::Multi(form) => raw_byte(c).is_some() || multi_char_bytes(c, form).is_some(),
        Family::Bytes => raw_byte(c).is_some() || set.tables.iter().any(|table| table.byte_of(c).is_some()),
    }
}

// ---------- Positions ----------

/// How many bytes of the file come before a position in the text, its byte-order mark included.
pub fn offset_of_position<S: AsRef<str>>(lines: &[S], eol_of: &dyn Fn(usize) -> Eol, line: usize, char_index: usize, format: &TextFormat) -> usize {
    let mut prefix: Vec<String> = lines.iter().take(line).map(|l| l.as_ref().to_string()).collect();
    if let Some(current) = lines.get(line) {
        prefix.push(current.as_ref().chars().take(char_index).collect());
    }
    encode_with(&prefix, eol_of, format, Lossy::Omit).map_or(0, |bytes| bytes.len())
}

/// The line and character at a byte offset, stepping back to the start of a split character.
pub fn position_of_offset(data: &[u8], offset: usize, format: &TextFormat) -> (usize, usize) {
    let mut end = offset.min(data.len());
    match format.charset.family {
        Family::Unicode(UnicodeForm::Utf8) => {
            while end > 0 && end < data.len() && (data[end] & 0xC0) == 0x80 {
                end -= 1;
            }
        }
        Family::Unicode(UnicodeForm::Utf16Le | UnicodeForm::Utf16Be) => end -= end % 2,
        Family::Unicode(UnicodeForm::Utf32Le | UnicodeForm::Utf32Be) => end -= end % 4,
        Family::Multi(_) | Family::Bytes => {}
    }
    let mut probe = format.clone();
    let decoded = decode(&data[..end], &mut probe);
    (decoded.lines.len() - 1, decoded.lines.last().map_or(0, |l| l.chars().count()))
}

/// Writes beside the file first, so a failed save can't damage the original.
pub fn write_file(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut temp_name = path.as_os_str().to_owned();
    temp_name.push(".limoniter-tmp");
    let temp = PathBuf::from(temp_name);
    let result = write_and_check(&temp, data).and_then(|()| fs::rename(&temp, path));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// The new file is on the disk and reads back correctly before it's allowed to replace the old one.
fn write_and_check(temp: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::{Read, Write};
    let mut file = fs::File::create(temp)?;
    file.write_all(data)?;
    file.sync_all()?;
    drop(file);
    let mut back = fs::File::open(temp)?;
    let mut chunk = vec![0u8; 1024 * 1024];
    let mut position = 0;
    loop {
        let got = back.read(&mut chunk)?;
        if got == 0 {
            break;
        }
        if data.get(position..position + got) != Some(&chunk[..got]) {
            return Err(std::io::Error::other("The saved copy read back differently, so the original file has been left alone."));
        }
        position += got;
    }
    if position == data.len() { Ok(()) } else { Err(std::io::Error::other("The saved copy came out too short, so the original file has been left alone.")) }
}

#[cfg(test)]
#[path = "codec_tests.rs"]
pub mod tests;
