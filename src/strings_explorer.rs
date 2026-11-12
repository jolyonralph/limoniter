//! The string explorer: finds the runs of readable ASCII in a file's bytes, and works out how each one is stored, from
//! the bytes round it: a length before it (1, 2 or 4 bytes, either way round), a zero byte after it, padding to a fixed
//! width that other strings in the file share, a last character with its top bit set (as many 8-bit machines' tables
//! end their words), a `$` at its end (CP/M's and DOS's print strings), or a line ending. Wide (UTF-16LE) strings can
//! be looked for too. It reads the bytes as they are in hex mode, and clicking a string selects it there, with its
//! length, terminator or padding.

use crate::panels::{Action, DocView};
use crate::theme;
use eframe::egui::{self, RichText, Ui};
use std::collections::HashMap;

/// More strings than this aren't listed, so a large file stays quick to show.
pub const MOST_FOUND: usize = 200_000;

/// How a string is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A count before the text: `width` bytes, in `big_endian` order or not. It counts the characters, the bytes of a
    /// wide string, or the characters and a zero after them.
    LengthPrefixed { width: u8, big_endian: bool, counts: Counts },
    /// A zero byte after the text (two for a wide string).
    NullTerminated,
    /// A field of `width` bytes that other strings in the file share, filled out with `pad`.
    Padded { width: usize, pad: u8 },
    /// The last character has its top bit set.
    HighBitEnd,
    /// Ends with `$`, as CP/M's and DOS's print function wants.
    DollarEnd,
    /// Ends with CR, LF or both.
    Line,
    /// Nothing round it says.
    Plain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Counts {
    Characters,
    Bytes,
    WithZero,
}

impl Kind {
    pub fn describe(self) -> String {
        match self {
            Kind::LengthPrefixed { width, big_endian, counts } => {
                let order = match (width, big_endian) {
                    (1, _) => String::new(),
                    (_, true) => ", big-endian".into(),
                    (_, false) => ", little-endian".into(),
                };
                let counting = match counts {
                    Counts::Characters => "",
                    Counts::Bytes => ", counting bytes",
                    Counts::WithZero => ", counting a zero at the end",
                };
                format!("Length first ({width} byte{}{order}{counting})", if width == 1 { "" } else { "s" })
            }
            Kind::NullTerminated => "Ends with a zero byte".into(),
            Kind::Padded { width, pad: b' ' } => format!("Fixed width {width}, padded with spaces"),
            Kind::Padded { width, pad: 0xA0 } => format!("Fixed width {width}, padded with shifted spaces ($A0)"),
            Kind::Padded { width, .. } => format!("Fixed width {width}, padded with zeros"),
            Kind::HighBitEnd => "Last character has its top bit set".into(),
            Kind::DollarEnd => "Ends with $ (CP/M and DOS)".into(),
            Kind::Line => "A line (ends with CR or LF)".into(),
            Kind::Plain => "No length or end marker".into(),
        }
    }

    /// The group the kind filter lists it under.
    pub fn group(self) -> &'static str {
        match self {
            Kind::LengthPrefixed { .. } => "Length first",
            Kind::NullTerminated => "Ends with a zero",
            Kind::Padded { .. } => "Fixed width",
            Kind::HighBitEnd => "Top bit ends it",
            Kind::DollarEnd => "Ends with $",
            Kind::Line => "Lines",
            Kind::Plain => "No marker",
        }
    }
}

const GROUPS: [&str; 7] = ["Length first", "Ends with a zero", "Fixed width", "Top bit ends it", "Ends with $", "Lines", "No marker"];

/// A string found in the bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Where its length starts, or its text when there's none before it.
    pub start: usize,
    /// One past its terminator or padding, or its text.
    pub end: usize,
    /// Where its text starts.
    pub text_start: usize,
    pub text: String,
    pub wide: bool,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// The fewest characters that count as a string.
    pub min_len: usize,
    /// Look for UTF-16LE strings as well.
    pub wide: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { min_len: 4, wide: false }
    }
}

fn printable(b: u8) -> bool {
    (0x20..0x7F).contains(&b) || b == b'\t'
}

/// Mostly letters, digits and spaces, with a few letters: what words look like, and machine code mostly doesn't.
fn looks_like_text(text: &str) -> bool {
    let letters = text.chars().filter(char::is_ascii_alphabetic).count();
    let good = text.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ').count();
    letters >= 3 && good * 4 >= text.chars().count() * 3
}

/// What a table ended by a top bit holds: BASIC's keywords and messages, in capitals.
fn looks_like_keyword(text: &str) -> bool {
    let letters = text.chars().filter(char::is_ascii_uppercase).count();
    letters >= 2 && letters * 2 >= text.chars().count() && text.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || " $#()<>=+-*/^,.'!?:;&".contains(c))
}

/// A length before `text_start`, of 4, 2 or 1 bytes, that counts `chars` characters (or `bytes` bytes). The widest
/// field whose top bytes are zero is taken, as a 1-byte count after zeros is more often a wider one's low byte.
fn length_before(bytes: &[u8], text_start: usize, chars: usize, text_bytes: usize, zero_after: bool) -> Option<Kind> {
    let read = |width: usize, big_endian: bool| -> Option<usize> {
        let field = bytes.get(text_start.checked_sub(width)?..text_start)?;
        Some(field.iter().enumerate().fold(0usize, |value, (i, b)| {
            let shift = if big_endian { 8 * (width - 1 - i) } else { 8 * i };
            value | (usize::from(*b) << shift)
        }))
    };
    for (width, big_endian) in [(4, false), (4, true), (2, false), (2, true), (1, false)] {
        let Some(value) = read(width, big_endian) else {
            continue;
        };
        // A big-endian count's leading zero straight after text is more likely that text's own terminator.
        if big_endian && width > 1 && text_start > width && printable(bytes[text_start - width - 1]) {
            continue;
        }
        let counts = if value == chars {
            Counts::Characters
        } else if value == text_bytes && text_bytes != chars {
            Counts::Bytes
        } else if zero_after && value == chars + 1 {
            Counts::WithZero
        } else {
            continue;
        };
        return Some(Kind::LengthPrefixed { width: width as u8, big_endian: big_endian && width > 1, counts });
    }
    None
}

/// Splits a run of space-padded fields that ran together, when it's whole fields of one width, each with text first
/// and a space last.
fn split_space_fields(run: &[u8]) -> Option<usize> {
    (4..=run.len() / 2).find(|&width| {
        run.len().is_multiple_of(width)
            && run.chunks(width).all(|field| field[0] != b' ' && field[width - 1] == b' ')
            && run.chunks(width).any(|field| field.iter().rev().take_while(|b| **b == b' ').count() >= 2)
    })
}

/// What follows a run of text says about it: a zero (or zeros), a `$`, a line ending, spaces to a width, or nothing.
fn by_what_follows(bytes: &[u8], i: usize, j: usize, min_len: usize) -> Found {
    let mut text: String = bytes[i..j].iter().map(|b| char::from(*b)).collect();
    let zeros = bytes[j..].iter().take_while(|b| **b == 0).count();
    let shifted_spaces = bytes[j..].iter().take_while(|b| **b == 0xA0).count();
    let mut end = j;
    let kind = if shifted_spaces > 0 {
        // Commodore pads names with shifted spaces: a field when others share its width.
        end += shifted_spaces;
        Kind::Padded { width: text.len() + shifted_spaces, pad: 0xA0 }
    } else if zeros > 0 {
        // A zero, or zeros to a fixed width; which, is worked out once every string is found.
        end += zeros;
        Kind::NullTerminated
    } else if text.ends_with('$') && text.len() > min_len && looks_like_text(&text[..text.len() - 1]) && text.trim().contains(' ') {
        text.pop();
        Kind::DollarEnd
    } else if let Some(dollar) = bytes[j..].iter().position(|b| !matches!(b, b'\r' | b'\n')).filter(|n| (1..=4).contains(n) && bytes[j + n] == b'$' && looks_like_text(&text) && text.trim().contains(' ')) {
        // DOS's messages end with their line ending and then the $.
        end += dollar + 1;
        Kind::DollarEnd
    } else if matches!(bytes.get(j), Some(b'\r' | b'\n')) {
        end += 1 + usize::from(bytes.get(j) == Some(&b'\r') && bytes.get(j + 1) == Some(&b'\n'));
        Kind::Line
    } else if text.ends_with("  ") && bytes.get(j).is_none_or(|b| !printable(*b)) {
        // Padded with spaces if other strings share its width; worked out once every string is found.
        Kind::Padded { width: text.len(), pad: b' ' }
    } else {
        Kind::Plain
    };
    Found { start: i, end, text_start: i, text, wide: false, kind }
}

/// A string as first read, and what it is if the first reading isn't borne out by the strings round it.
struct Candidate {
    found: Found,
    otherwise: Option<Found>,
    /// Believed without others like it nearby.
    trusted: bool,
}

/// Every string in `bytes` of at least `min_len` characters, in order, with how each is stored.
pub fn find(bytes: &[u8], options: &Options) -> Vec<Found> {
    let min_len = options.min_len.max(2);
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut i = 0;
    while i < bytes.len() && candidates.len() < MOST_FOUND {
        if !printable(bytes[i]) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < bytes.len() && printable(bytes[j]) {
            j += 1;
        }
        // A last character with its top bit set ends it, when the character under the bit is a readable one and
        // the whole is the kind of thing such tables hold.
        // ($A0 is Commodore's shifted space, which pads names instead.)
        // A capital with its top bit set after a space starts a word, as PETSCII text has them; it doesn't end one.
        let high_end = bytes.get(j).is_some_and(|b| *b > 0xA0 && *b < 0xFF) && bytes[j - 1] != b' ' && {
            let mut text: String = bytes[i..j].iter().map(|b| char::from(*b)).collect();
            text.push(char::from(bytes[j] & 0x7F));
            looks_like_keyword(&text)
        };
        let chars = j - i + usize::from(high_end);
        // Fewer characters than asked for, or nothing but spaces and punctuation, isn't a string.
        if chars < min_len || !bytes[i..j].iter().any(u8::is_ascii_alphanumeric) {
            i = j.max(i + 1);
            continue;
        }
        let run = &bytes[i..j];
        if !high_end && let Some(width) = split_space_fields(run) {
            for (n, field) in run.chunks(width).enumerate() {
                let at = i + n * width;
                let text = String::from_utf8_lossy(field).trim_end().to_string();
                candidates.push(Candidate { found: Found { start: at, end: at + width, text_start: at, text, wide: false, kind: Kind::Padded { width, pad: b' ' } }, otherwise: None, trusted: true });
            }
            i = j;
            continue;
        }
        let base = by_what_follows(bytes, i, j, min_len);
        if high_end {
            let mut text: String = run.iter().map(|b| char::from(*b)).collect();
            text.push(char::from(bytes[j] & 0x7F));
            candidates.push(Candidate { found: Found { start: i, end: j + 1, text_start: i, text, wide: false, kind: Kind::HighBitEnd }, otherwise: Some(base), trusted: false });
            i = j + 1;
            continue;
        }
        // A count before the text, or as its own first character when that's a readable one.
        let zero_after = bytes.get(j) == Some(&0);
        let text_start = if length_before(bytes, i, chars, chars, zero_after).is_none() && usize::from(bytes[i]) == chars - 1 && chars > min_len { i + 1 } else { i };
        let prefixed = length_before(bytes, text_start, j - text_start, j - text_start, zero_after).map(|kind| {
            let width = match kind {
                Kind::LengthPrefixed { width, .. } => usize::from(width),
                _ => 0,
            };
            let end = j + usize::from(matches!(kind, Kind::LengthPrefixed { counts: Counts::WithZero, .. }));
            Found { start: text_start - width, end, text_start, text: bytes[text_start..j].iter().map(|b| char::from(*b)).collect(), wide: false, kind }
        });
        // A count is believed on its own only when it's four bytes before what looks like words; a byte or two that
        // happens to equal a length is common (a "\n" and a zero before a string of ten is one), so those need others
        // like them nearby, as a table of Pascal strings has.
        candidates.push(match prefixed {
            Some(found) => {
                let trusted = text_start == i && matches!(found.kind, Kind::LengthPrefixed { width: 4, .. }) && looks_like_text(&found.text);
                Candidate { found, otherwise: Some(base), trusted }
            }
            None => Candidate { found: base, otherwise: None, trusted: true },
        });
        i = j;
    }
    let mut found = settle(candidates);
    if options.wide {
        found.extend(find_wide(bytes, min_len));
        found.sort_by_key(|f| (f.start, f.wide));
        found.truncate(MOST_FOUND);
    }
    found
}

/// Every string in what hex mode has open. A file left on disk is read a stretch at a time, each with a little
/// more after it so that a string across the join is read whole; the next stretch starts after the last one kept.
pub fn find_in(hex: &crate::hex_buffer::HexBuffer, options: &Options) -> Vec<Found> {
    const STRETCH: usize = 32 * 1024 * 1024;
    const OVERLAP: usize = 64 * 1024;
    if hex.len() <= STRETCH + OVERLAP {
        return find(&hex.read(0, hex.len()), options);
    }
    let mut found = Vec::new();
    let mut at = 0;
    while at < hex.len() && found.len() < MOST_FOUND {
        let bytes = hex.read(at, STRETCH + OVERLAP);
        let cut = at + STRETCH;
        let mut next = cut;
        for mut f in find(&bytes, options) {
            f.start += at;
            f.end += at;
            f.text_start += at;
            if f.start >= cut && at + bytes.len() < hex.len() {
                break;
            }
            next = next.max(f.end);
            found.push(f);
        }
        at = next;
    }
    found.truncate(MOST_FOUND);
    found
}

/// UTF-16LE strings: readable ASCII characters, each with a zero byte after it, at either alignment.
fn find_wide(bytes: &[u8], min_len: usize) -> Vec<Found> {
    let mut found = Vec::new();
    for parity in 0..2 {
        let mut i = parity;
        while i + 1 < bytes.len() && found.len() < MOST_FOUND {
            let mut j = i;
            while j + 1 < bytes.len() && printable(bytes[j]) && bytes[j + 1] == 0 {
                j += 2;
            }
            let chars = (j - i) / 2;
            if chars < min_len || !bytes[i..j].iter().step_by(2).any(u8::is_ascii_alphanumeric) {
                i = if j == i { i + 2 } else { j };
                continue;
            }
            let text: String = bytes[i..j].iter().step_by(2).map(|b| char::from(*b)).collect();
            let terminated = bytes.get(j..j + 2) == Some(&[0, 0]);
            let (kind, start, end) = match length_before(bytes, i, chars, chars * 2, terminated) {
                Some(kind @ Kind::LengthPrefixed { width, counts, .. }) => (kind, i - usize::from(width), j + if counts == Counts::WithZero { 2 } else { 0 }),
                _ if terminated => (Kind::NullTerminated, i, j + 2),
                _ => (Kind::Plain, i, j),
            };
            found.push(Found { start, end, text_start: i, text, wide: true, kind });
            i = j;
        }
    }
    found
}

/// What can only be told from the whole file.
/// - A count of one byte before a string is a Pascal string's only when there are others of its kind nearby; a byte
///   that happens to equal the length is common in machine code.
/// - A last character with its top bit set ends a string only in a table of them, a few bytes apart; otherwise that
///   byte is just what came next.
/// - Strings followed by zeros are fields of a fixed width when at least three come one after another at the same
///   spacing, as a table or an array of records has them; strings a processor pads to even addresses don't.
/// - Strings ending in spaces are padded with them when at least three share the width.
fn settle(candidates: Vec<Candidate>) -> Vec<Found> {
    // Chains of three or more of a kind, one after another a few bytes apart (eight for a top-bit table, whose short
    // words are too short to be listed, and 32 for counted strings), most of them looking like words.
    let gap = |a: &Found, b: &Found, most: usize| b.start >= a.end && b.start - a.end <= most;
    let alike = |a: &Kind, b: &Kind| match (a, b) {
        (Kind::LengthPrefixed { .. }, Kind::LengthPrefixed { .. }) => true,
        (a, b) => a == b,
    };
    let mut borne_out: Vec<bool> = candidates.iter().map(|c| c.trusted || c.otherwise.is_none()).collect();
    let mut k = 0;
    while k < candidates.len() {
        if borne_out[k] {
            k += 1;
            continue;
        }
        let kind = candidates[k].found.kind;
        let most = if kind == Kind::HighBitEnd { 8 } else { 32 };
        let mut last = k;
        while last + 1 < candidates.len() && !candidates[last + 1].trusted && alike(&candidates[last + 1].found.kind, &kind) && gap(&candidates[last].found, &candidates[last + 1].found, most) {
            last += 1;
        }
        let chain = &candidates[k..=last];
        let wordy = kind == Kind::HighBitEnd || chain.iter().filter(|c| looks_like_text(&c.found.text)).count() * 2 > chain.len();
        if chain.len() >= 3 && wordy {
            borne_out[k..=last].iter_mut().for_each(|b| *b = true);
        }
        k = last + 1;
    }
    let mut found: Vec<Found> = candidates
        .into_iter()
        .zip(borne_out)
        .map(|(c, kept)| if kept { c.found } else { c.otherwise.unwrap_or(c.found) })
        .collect();
    // Names padded with spaces or shifted spaces: records, three or more one after another at the same spacing, with
    // names of different lengths padded to the one width, as a 1541's directory has them.
    let mut groups: HashMap<(usize, u8), Vec<usize>> = HashMap::new();
    for (i, f) in found.iter().enumerate() {
        if let Kind::Padded { width, pad: pad @ (b' ' | 0xA0) } = f.kind {
            groups.entry((width, pad)).or_default().push(i);
        }
    }
    let mut fields = vec![false; found.len()];
    for members in groups.values() {
        let lengths: std::collections::HashSet<usize> = members.iter().map(|&i| found[i].text.trim_end().len()).collect();
        if members.len() < 3 || lengths.len() < 2 {
            continue;
        }
        for run in members.windows(3) {
            let (a, b, c) = (found[run[0]].start, found[run[1]].start, found[run[2]].start);
            if b - a == c - b && b - a <= 4096 {
                run.iter().for_each(|&i| fields[i] = true);
            }
        }
    }
    for (f, field) in found.iter_mut().zip(fields) {
        if let Kind::Padded { pad: b' ' | 0xA0, .. } = f.kind {
            if field {
                f.text.truncate(f.text.trim_end().len());
            } else {
                // Not a field after all: the text stands as it is, without what came after it.
                f.kind = Kind::Plain;
                f.end = f.text_start + f.text.len();
            }
        }
    }
    // Strings of different lengths that each run on in zeros to the same width are fields of that width.
    let mut by_width: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, f) in found.iter().enumerate() {
        if f.kind == Kind::NullTerminated && f.end - f.text_start > f.text.len() + 1 {
            by_width.entry(f.end - f.start).or_default().push(i);
        }
    }
    let mut padded = vec![None; found.len()];
    for (width, members) in by_width {
        // Lengths a few apart can all reach one width by a processor's alignment; a field's names vary more.
        let shortest = members.iter().map(|&i| found[i].text.len()).min().unwrap_or(0);
        let longest = members.iter().map(|&i| found[i].text.len()).max().unwrap_or(0);
        let wordy = members.iter().filter(|&&i| looks_like_text(&found[i].text)).count() * 2 > members.len();
        if members.len() >= 3 && longest - shortest >= 3 && wordy {
            for i in members {
                padded[i] = Some(width);
            }
        }
    }
    for (f, width) in found.iter_mut().zip(padded) {
        if f.kind != Kind::NullTerminated {
            continue;
        }
        match width {
            Some(width) => {
                f.kind = Kind::Padded { width, pad: 0 };
                f.end = f.start + width;
            }
            // Only the one zero belongs to it.
            None => f.end = f.text_start + f.text.len() + 1,
        }
    }
    found
}

/// How many strings of each group there are, for the summary.
pub fn summary(found: &[Found]) -> String {
    if found.is_empty() {
        return "No strings found.".into();
    }
    let mut counts: Vec<(&str, usize)> = GROUPS.iter().map(|g| (*g, found.iter().filter(|f| f.kind.group() == *g).count())).filter(|(_, n)| *n > 0).collect();
    counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let more = if found.len() >= MOST_FOUND { " (the first 200,000)" } else { "" };
    format!("{} strings{more}: {}", found.len(), counts.iter().map(|(g, n)| format!("{n} {}", g.to_lowercase())).collect::<Vec<_>>().join(", "))
}

/// The panel's settings and what it found last.
#[derive(Default)]
pub struct PanelState {
    options: Options,
    filter: String,
    group: Option<&'static str>,
    found: Vec<Found>,
    found_for: Option<(u64, u64, Options)>,
    selected: Option<usize>,
}

pub fn panel(ui: &mut Ui, state: &mut PanelState, doc: &DocView<'_>, actions: &mut Vec<Action>) {
    if doc.no_file {
        ui.label(RichText::new("No file open").color(theme::text_faint()));
        return;
    }
    if !doc.hex_mode {
        ui.label(RichText::new("The string explorer looks through the file's bytes, so it works in hex mode.").color(theme::text_faint()).size(12.5));
        if ui.button("Show as hex").clicked() {
            actions.push(Action::ViewHex);
        }
        return;
    }
    ui.horizontal(|ui| {
        ui.label("At least");
        ui.add(egui::DragValue::new(&mut state.options.min_len).range(3..=64)).on_hover_text("The fewest characters in a row that count as a string");
        ui.label("characters");
    });
    ui.checkbox(&mut state.options.wide, "Wide strings (UTF-16)").on_hover_text("Also look for text with a zero byte after each character, as Windows keeps it");
    let key = (doc.id, doc.hex.version(), state.options);
    // A large file is looked through again after an edit only when asked, so typing in it stays quick.
    let edited_large = state.found_for.is_some_and(|(id, _, options)| id == doc.id && options == state.options) && doc.hex.len() > 16 * 1024 * 1024;
    if state.found_for != Some(key) && edited_large {
        ui.horizontal(|ui| {
            ui.label(RichText::new("The file has changed since it was looked through.").color(theme::text_faint()).size(12.0));
            if ui.button("Look again").clicked() {
                state.found_for = None;
            }
        });
    }
    if state.found_for != Some(key) && !(edited_large && state.found_for.is_some()) {
        state.found = find_in(doc.hex, &state.options);
        state.found_for = Some(key);
        state.selected = None;
    }
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("strings-kind").selected_text(state.group.unwrap_or("Every kind")).width(130.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut state.group, None, "Every kind");
            for group in GROUPS {
                ui.selectable_value(&mut state.group, Some(group), group);
            }
        });
        ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text("Containing\u{2026}").desired_width(f32::INFINITY));
    });
    ui.label(RichText::new(summary(&state.found)).color(theme::text_faint()).size(12.0));
    ui.label(RichText::new("Click one to select it in the hex, with its length, end or padding.").color(theme::text_faint()).size(12.0));
    let filter = state.filter.to_lowercase();
    let shown: Vec<usize> = state.found.iter().enumerate().filter(|(_, f)| state.group.is_none_or(|g| f.kind.group() == g) && (filter.is_empty() || f.text.to_lowercase().contains(&filter))).map(|(i, _)| i).collect();
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 4.0;
    egui::ScrollArea::both().id_salt("strings-scroll").auto_shrink([false, false]).show_rows(ui, row_height, shown.len(), |ui, range| {
        for &index in &shown[range] {
            let f = &state.found[index];
            let words = format!("{:08X}  {}{}", f.start, if f.wide { "W " } else { "" }, f.text);
            let response = ui.selectable_label(state.selected == Some(index), RichText::new(words).monospace()).on_hover_text(f.kind.describe());
            if response.clicked() {
                state.selected = Some(index);
                actions.push(Action::SelectBytes { start: f.start, end: f.end });
            }
            response.context_menu(|ui| {
                if ui.button("Copy the text").clicked() {
                    actions.push(Action::CopyText(f.text.clone()));
                    ui.close();
                }
                if ui.button("Copy the offset").clicked() {
                    actions.push(Action::CopyText(format!("{:X}", f.start)));
                    ui.close();
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(bytes: &[u8]) -> Found {
        let found = find(bytes, &Options::default());
        assert_eq!(found.len(), 1, "{found:?}");
        found.into_iter().next().unwrap()
    }

    #[test]
    fn a_zero_after_the_text_ends_it() {
        let f = one(b"\x01\x02Hello world\x00\x8F\x90");
        assert_eq!((f.text.as_str(), f.kind, f.start, f.end), ("Hello world", Kind::NullTerminated, 2, 14));
    }

    #[test]
    fn a_length_before_the_text_is_found_whatever_its_width_and_order() {
        // Counts are believed when there's another counted string nearby, as in a table of Pascal strings or a Java
        // class file's constants; a four-byte count before words is believed on its own.
        let cases: [(&[u8], Kind); 6] = [
            (b"\x90\x05Hello\x90\x04Next\x90\x05Third\x90", Kind::LengthPrefixed { width: 1, big_endian: false, counts: Counts::Characters }),
            (b"\x90\x05\x00Hello\x04\x00Next\x05\x00Third\x90", Kind::LengthPrefixed { width: 2, big_endian: false, counts: Counts::Characters }),
            (b"\x01\x00\x05Hello\x01\x00\x04Next\x01\x00\x05Third\x90", Kind::LengthPrefixed { width: 2, big_endian: true, counts: Counts::Characters }),
            (b"\x05\x00\x00\x00Hello\x90", Kind::LengthPrefixed { width: 4, big_endian: false, counts: Counts::Characters }),
            (b"\x90\x00\x00\x00\x05Hello\x90", Kind::LengthPrefixed { width: 4, big_endian: true, counts: Counts::Characters }),
            (b"\x90\x06Hello\x00\x05Next\x00\x06Third\x00\x90", Kind::LengthPrefixed { width: 1, big_endian: false, counts: Counts::WithZero }),
        ];
        for (bytes, kind) in cases {
            let f = find(bytes, &Options::default()).into_iter().next().unwrap();
            assert_eq!((f.text.as_str(), f.kind), ("Hello", kind), "{bytes:?}");
            let width = match kind {
                Kind::LengthPrefixed { width, .. } => usize::from(width),
                _ => 0,
            };
            assert_eq!(f.text_start - f.start, width, "the selection starts at the length");
        }
        // A Pascal string whose count is itself a readable character: '%' is 37.
        let mut bytes = vec![0x90, b'%'];
        bytes.extend([b'x'; 37]);
        bytes.extend(b"\x05Hello\x90\x04Next\x90");
        let f = &find(&bytes, &Options::default())[0];
        assert_eq!((f.text.len(), f.text_start - f.start), (37, 1));
        // A byte or two before a lone string that happens to equal its length isn't taken for a count: here a
        // string ending in a line feed, and its zero, before one of ten characters.
        let f = one(b"\x90\x05Hello\x90");
        assert_eq!((f.kind, f.start), (Kind::Plain, 2));
        let found = find(b"\x90\x91Bad command\n\x00ECHO is %s\x00", &Options::default());
        assert_eq!(found.iter().map(|f| f.kind).collect::<Vec<_>>(), [Kind::Line, Kind::NullTerminated]);
    }

    #[test]
    fn fields_of_one_width_are_padded_and_a_lone_string_just_ends_with_a_zero() {
        // A table of 12-byte names, padded with zeros.
        let mut table = Vec::new();
        for name in ["ABLE", "BRAVO", "CHARLIE", "DELTA"] {
            let mut field = name.as_bytes().to_vec();
            field.resize(12, 0);
            table.extend(field);
        }
        table.extend(b"\x90\x91odd one\x00\x00\x00\x90");
        let found = find(&table, &Options::default());
        assert_eq!(found.iter().map(|f| f.text.as_str()).collect::<Vec<_>>(), ["ABLE", "BRAVO", "CHARLIE", "DELTA", "odd one"]);
        assert!(found[..4].iter().all(|f| f.kind == Kind::Padded { width: 12, pad: 0 } && f.end - f.start == 12));
        assert_eq!(found[4].kind, Kind::NullTerminated);
        assert_eq!(found[4].end - found[4].start, 8, "only its one zero goes with it");
        // Space-padded fields that run together are split back into fields.
        let found = find(b"\x90ALICE     BOB       CAROL     \x90", &Options::default());
        assert_eq!(found.iter().map(|f| f.text.as_str()).collect::<Vec<_>>(), ["ALICE", "BOB", "CAROL"]);
        assert!(found.iter().all(|f| f.kind == Kind::Padded { width: 10, pad: b' ' }));
    }

    #[test]
    fn strings_a_processor_pads_to_even_addresses_are_not_taken_for_fields() {
        // 68000 messages: each ends with a zero, and odd ones with another to keep the next on an even address.
        let found = find(b"Out of memory!\x00\x00Can't open file!\x00\x00Read/Write Error\x00\x00Need more\x00", &Options::default());
        assert!(found.iter().all(|f| f.kind == Kind::NullTerminated), "{found:?}");
        assert!(find(b"\x90                \x00\x90----\x00", &Options::default()).is_empty(), "only spaces and dashes");
    }

    #[test]
    fn commodore_names_padded_with_shifted_spaces_are_fields() {
        // Three directory entries as a 1541 writes them: 32 bytes apart, each name padded to 16 with $A0.
        let mut directory = Vec::new();
        for name in ["HELLO", "GAME LOADER", "NOTES"] {
            let mut entry = vec![0x00, 0x00, 0x82, 0x11, 0x00];
            let mut field = name.as_bytes().to_vec();
            field.resize(16, 0xA0);
            entry.extend(field);
            entry.resize(32, 0x00);
            directory.extend(entry);
        }
        let found = find(&directory, &Options::default());
        assert_eq!(found.iter().map(|f| (f.text.as_str(), f.kind)).collect::<Vec<_>>(), ["HELLO", "GAME LOADER", "NOTES"].map(|n| (n, Kind::Padded { width: 16, pad: 0xA0 })));
        assert_eq!(found[0].end - found[0].start, 16);
    }

    #[test]
    fn retro_endings_are_recognised() {
        // A keyword table with the top bit set on each word's last letter, as the Spectrum's ROM has it.
        let found = find(b"\x00PRIN\xD4LIS\xD4CLEA\xD2\x00", &Options::default());
        assert_eq!(found.iter().map(|f| (f.text.as_str(), f.kind)).collect::<Vec<_>>(), [("PRINT", Kind::HighBitEnd), ("LIST", Kind::HighBitEnd), ("CLEAR", Kind::HighBitEnd)]);
        // PETSCII text has its capitals' top bits set at the start of words, which isn't the end of anything.
        assert!(find(b"\x90DIE \xCCEHRE \xD6ON \xC4ER\x00", &Options::default()).iter().all(|f| f.kind != Kind::HighBitEnd));
        // DOS's print strings, with and without a line ending, and with code after that has its top bit set.
        let f = one(b"\xB4\x09\xCD\x21\xC3Program too big\r\n$\x90");
        assert_eq!((f.text.as_str(), f.kind, f.end), ("Program too big", Kind::DollarEnd, 23));
        let f = one(b"\x90\x91All done here$\xC3\x90");
        assert_eq!((f.text.as_str(), f.kind), ("All done here", Kind::DollarEnd), "a lone top-bit byte after it isn't its end");
        let f = one(b"\x90\x91Some text\xE8\x10\x00");
        assert_eq!((f.text.as_str(), f.kind), ("Some text", Kind::Plain));
        // Spaces at the end are only padding when other strings share the width.
        let f = one(b"\x90\x91trailing  \x90");
        assert_eq!((f.text.as_str(), f.kind), ("trailing  ", Kind::Plain));
        let found = find(b"\x90NAME    \x90\x91CITY    \x90\x91ZIP     \x90\x91", &Options::default());
        assert!(found.iter().all(|f| f.kind == Kind::Padded { width: 8, pad: b' ' }), "{found:?}");
        assert_eq!(found[2].text, "ZIP");
        let found = find(b"first line\r\nsecond line\n", &Options::default());
        assert_eq!(found.iter().map(|f| (f.text.as_str(), f.kind, f.end)).collect::<Vec<_>>(), [("first line", Kind::Line, 12), ("second line", Kind::Line, 24)]);
        let f = one(b"\x90\x91no marker\x92");
        assert_eq!(f.kind, Kind::Plain);
        assert!(find(b"\x90abc\x90", &Options::default()).is_empty(), "shorter than the least");
    }

    #[test]
    fn wide_strings_are_found_when_asked_for() {
        let mut bytes = vec![0x90, 0x05, 0x00];
        bytes.extend("Hello".encode_utf16().flat_map(u16::to_le_bytes));
        bytes.extend([0x90, 0x91]);
        bytes.extend("Windows\0".encode_utf16().flat_map(u16::to_le_bytes));
        assert!(find(&bytes, &Options::default()).is_empty());
        let found = find(&bytes, &Options { min_len: 4, wide: true });
        assert_eq!(found.iter().map(|f| (f.text.as_str(), f.wide, f.kind)).collect::<Vec<_>>(), [("Hello", true, Kind::LengthPrefixed { width: 2, big_endian: false, counts: Counts::Characters }), ("Windows", true, Kind::NullTerminated)]);
        assert_eq!(found[1].end - found[1].text_start, 16, "with its two zero bytes");
    }

    #[test]
    fn the_summary_counts_each_kind() {
        let found = find(b"Hello\x00world\x00\x90\x05Pasca\x05Lists\x05Third\x90\x91Plain text\x90", &Options::default());
        assert_eq!(summary(&found), "6 strings: 3 length first, 2 ends with a zero, 1 no marker");
        assert_eq!(summary(&[]), "No strings found.");
    }
}

#[cfg(test)]
mod real_files {
    use super::*;
    use std::sync::Arc;

    /// A file from inside an image, or images inside images, in ../testmedia.
    fn take(image: &str, path: &[&str]) -> Option<Vec<u8>> {
        let mut volume = crate::media::open(Arc::new(std::fs::read(image).ok()?), image).ok()?;
        let mut dir: Option<crate::media::Entry> = None;
        for (n, part) in path.iter().enumerate() {
            let entry = volume.list(dir.as_ref()).ok()?.into_iter().find(|e| e.name.eq_ignore_ascii_case(part))?;
            if n + 1 == path.len() {
                return volume.read(&entry).ok();
            }
            if entry.is_dir {
                dir = Some(entry);
            } else {
                volume = crate::media::open(Arc::new(volume.read(&entry).ok()?), part).ok()?;
                dir = None;
            }
        }
        None
    }

    /// Real programs give no lengths, top-bit endings, fields or $ endings by coincidence, and a real Commodore
    /// directory's names are found as the fields they are.
    #[test]
    fn real_programs_and_disks_are_read_as_they_are() {
        let programs = [
            take("../testmedia/amiga-floppy-fredfish-801/amiga-floppy-fredfish-801.adf", &["c", "MuchMore"]),
            take("../testmedia/FD12FLOPPY/FD12FLOPPY.zip", &["FLOPPY.img", "COMMAND.COM"]),
        ];
        for bytes in programs.into_iter().flatten() {
            let found = find(&bytes, &Options::default());
            assert!(found.len() > 200);
            let odd: Vec<&Found> = found.iter().filter(|f| !matches!(f.kind, Kind::NullTerminated | Kind::Plain | Kind::Line)).collect();
            assert!(odd.is_empty(), "{odd:?}");
        }
        let Ok(disk) = std::fs::read("../testmedia/Reiners_PD_Nr._17_19xx_Reiner_Public_Domain_de/Reiner's PD Nr. 17 (19xx)(Reiner Public Domain)(de)(Side A).d64") else {
            return;
        };
        let found = find(&disk, &Options::default());
        // Track 18's directory, at 0x16600, as c1541 lists it.
        let directory: Vec<&str> = found.iter().filter(|f| (0x16600..0x16700).contains(&f.start) && f.kind == (Kind::Padded { width: 16, pad: 0xA0 })).map(|f| f.text.as_str()).collect();
        assert_eq!(directory, ["MENU", "VORWORT", "BYTESTAR SPEED", "ERKLAERUNG"]);
    }
}
