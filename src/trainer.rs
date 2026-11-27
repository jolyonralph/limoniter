//! The trainer: finding where a game keeps its lives, its score or its energy, as cheat finders do. Take a
//! snapshot of memory, play on, and narrow the addresses down by what happened to their values (changed, not
//! changed, went up, went down, equals a number) until a few are left; then freeze one at a value, so it's
//! written back every frame, or block writes to it where the machine can. What's frozen is kept for each program.
//! This module is the logic, with no screen or machine in it; `trainer_host` shows it and reaches the machines.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How wide a value is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Width {
    #[default]
    Byte,
    /// Two bytes, in the order the machine keeps them.
    Word,
}

impl Width {
    pub fn bytes(self) -> usize {
        match self {
            Width::Byte => 1,
            Width::Word => 2,
        }
    }
}

/// What a search asks of each candidate address, against the last snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Search {
    Changed,
    NotChanged,
    WentUp,
    WentDown,
    Equals(u32),
}

/// A value of `width` at `address` in memory that starts at `base`, if it's all there.
pub fn read(memory: &[u8], base: u32, address: u32, width: Width, big_endian: bool) -> Option<u32> {
    let at = usize::try_from(address.checked_sub(base)?).ok()?;
    let bytes = memory.get(at..at + width.bytes())?;
    Some(match (width, big_endian) {
        (Width::Byte, _) => u32::from(bytes[0]),
        (Width::Word, false) => u32::from(u16::from_le_bytes([bytes[0], bytes[1]])),
        (Width::Word, true) => u32::from(u16::from_be_bytes([bytes[0], bytes[1]])),
    })
}

/// A value as the bytes to write, in the machine's order, cut to the width.
pub fn bytes_of(value: u32, width: Width, big_endian: bool) -> Vec<u8> {
    match (width, big_endian) {
        (Width::Byte, _) => vec![value as u8],
        (Width::Word, false) => (value as u16).to_le_bytes().to_vec(),
        (Width::Word, true) => (value as u16).to_be_bytes().to_vec(),
    }
}

/// The addresses still thought to hold what's being looked for.
#[derive(Clone, Debug, Default)]
pub struct Finder {
    /// Where memory (as read) starts.
    pub base: u32,
    pub width: Width,
    pub big_endian: bool,
    /// The memory at the last snapshot, and what the newest search compared with (which is the snapshot before it).
    last: Vec<u8>,
    compared: Vec<u8>,
    candidates: Vec<u32>,
    started: bool,
}

impl Finder {
    /// Takes a snapshot, with every address a candidate.
    pub fn start(&mut self, base: u32, memory: &[u8]) {
        self.base = base;
        self.last = memory.to_vec();
        self.compared = memory.to_vec();
        let step = self.width.bytes();
        self.candidates = (0..memory.len().saturating_sub(step - 1)).map(|i| base + i as u32).collect();
        self.started = true;
    }

    pub fn started(&self) -> bool {
        self.started
    }

    /// How many addresses are left.
    pub fn count(&self) -> usize {
        self.candidates.len()
    }

    pub fn candidates(&self) -> &[u32] {
        &self.candidates
    }

    /// The value an address had when the last search compared it: what it was "before".
    pub fn before(&self, address: u32) -> Option<u32> {
        read(&self.compared, self.base, address, self.width, self.big_endian)
    }

    /// Narrows the candidates by comparing memory now with the last snapshot, which then becomes `memory`. How
    /// many are left.
    pub fn narrow(&mut self, search: Search, memory: &[u8]) -> usize {
        if !self.started || memory.len() != self.last.len() {
            return self.candidates.len();
        }
        let (base, width, big) = (self.base, self.width, self.big_endian);
        let last = &self.last;
        self.candidates.retain(|&address| {
            let (Some(then), Some(now)) = (read(last, base, address, width, big), read(memory, base, address, width, big)) else {
                return false;
            };
            match search {
                Search::Changed => now != then,
                Search::NotChanged => now == then,
                Search::WentUp => now > then,
                Search::WentDown => now < then,
                Search::Equals(wanted) => now == wanted,
            }
        });
        self.compared = std::mem::replace(&mut self.last, memory.to_vec());
        self.candidates.len()
    }

    /// Changes how wide a value is, which starts the search again (the snapshot's candidates mean something else).
    pub fn set_width(&mut self, width: Width) {
        if self.width != width {
            self.width = width;
            self.started = false;
            self.candidates.clear();
        }
    }
}

/// An address held at a value, or its writes blocked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Freeze {
    pub address: u32,
    pub value: u32,
    pub width: Width,
    pub name: String,
    pub on: bool,
    /// Writes are stopped, where the machine can, rather than the value written back each frame.
    pub block: bool,
}

impl Freeze {
    pub fn bytes(&self, big_endian: bool) -> Vec<u8> {
        bytes_of(self.value, self.width, big_endian)
    }
}

/// What's frozen, for each program (by its name), kept in a file beside the settings.
#[derive(Clone, Debug, Default)]
pub struct Kept {
    pub programs: BTreeMap<String, Vec<Freeze>>,
    path: Option<PathBuf>,
}

impl Kept {
    pub fn load(path: Option<PathBuf>) -> Kept {
        let programs = path.as_deref().and_then(|p| std::fs::read_to_string(p).ok()).map(|text| parse(&text)).unwrap_or_default();
        Kept { programs, path }
    }

    pub fn of(&self, program: &str) -> &[Freeze] {
        self.programs.get(program).map_or(&[], Vec::as_slice)
    }

    /// Replaces a program's freezes, and writes them out if that changed anything.
    pub fn set(&mut self, program: &str, freezes: Vec<Freeze>) {
        if self.of(program) == freezes.as_slice() {
            return;
        }
        if freezes.is_empty() {
            self.programs.remove(program);
        } else {
            self.programs.insert(program.to_string(), freezes);
        }
        self.save();
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let _ = std::fs::write(path, text_of(&self.programs));
    }

    pub fn file_in(folder: &Path) -> PathBuf {
        folder.join("trainer.tsv")
    }
}

/// One line each: program, address (hex), value, width (1 or 2), block (0 or 1), on (0 or 1), name. Tabs and
/// new lines in the names are turned into spaces.
fn text_of(programs: &BTreeMap<String, Vec<Freeze>>) -> String {
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    let mut text = String::new();
    for (program, freezes) in programs {
        for f in freezes {
            text.push_str(&format!("{}\t{:X}\t{}\t{}\t{}\t{}\t{}\n", clean(program), f.address, f.value, f.width.bytes(), u8::from(f.block), u8::from(f.on), clean(&f.name)));
        }
    }
    text
}

fn parse(text: &str) -> BTreeMap<String, Vec<Freeze>> {
    let mut programs: BTreeMap<String, Vec<Freeze>> = BTreeMap::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 7 {
            continue;
        }
        let (Ok(address), Ok(value)) = (u32::from_str_radix(parts[1], 16), parts[2].parse::<u32>()) else {
            continue;
        };
        let width = if parts[3] == "2" { Width::Word } else { Width::Byte };
        programs.entry(parts[0].to_string()).or_default().push(Freeze { address, value, width, block: parts[4] == "1", on: parts[5] == "1", name: parts[6].to_string() });
    }
    programs
}

/// The memory a frozen address is held in: the writes to make now, as address and bytes, for each that's on and
/// not merely blocked.
pub fn writes_due(freezes: &[Freeze], big_endian: bool) -> Vec<(u32, Vec<u8>)> {
    freezes.iter().filter(|f| f.on).map(|f| (f.address, f.bytes(big_endian))).collect()
}

/// A number typed, as decimal or as hex with `$` or `0x` in front.
pub fn parse_number(text: &str) -> Option<u32> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix('$').or_else(|| text.strip_prefix("0x")).or_else(|| text.strip_prefix("0X")) {
        return u32::from_str_radix(hex, 16).ok();
    }
    text.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory(values: &[u8]) -> Vec<u8> {
        values.to_vec()
    }

    fn finder(base: u32, first: &[u8]) -> Finder {
        let mut finder = Finder::default();
        finder.start(base, first);
        finder
    }

    #[test]
    fn starting_makes_every_address_a_candidate() {
        let f = finder(0x4000, &[0; 16]);
        assert_eq!(f.count(), 16);
        assert_eq!(f.candidates()[0], 0x4000);
    }

    #[test]
    fn each_kind_of_search_narrows_by_the_last_snapshot() {
        let start = memory(&[5, 5, 5, 5, 5]);
        // Changed: those that differ from the snapshot.
        let mut f = finder(0x80, &start);
        assert_eq!(f.narrow(Search::Changed, &[5, 6, 4, 5, 9]), 3);
        assert_eq!(f.candidates(), [0x81, 0x82, 0x84]);
        // Went up, after that, against the second snapshot.
        assert_eq!(f.narrow(Search::WentUp, &[5, 7, 3, 5, 9]), 1);
        assert_eq!(f.candidates(), [0x81]);
        // Not changed: those the same.
        let mut f = finder(0x80, &start);
        assert_eq!(f.narrow(Search::NotChanged, &[5, 6, 4, 5, 9]), 2);
        assert_eq!(f.candidates(), [0x80, 0x83]);
        // Went down.
        let mut f = finder(0x80, &start);
        assert_eq!(f.narrow(Search::WentDown, &[5, 6, 4, 5, 9]), 1);
        assert_eq!(f.candidates(), [0x82]);
        // Equals a number, whatever it was before.
        let mut f = finder(0x80, &start);
        assert_eq!(f.narrow(Search::Equals(9), &[9, 6, 4, 5, 9]), 2);
        assert_eq!(f.candidates(), [0x80, 0x84]);
    }

    #[test]
    fn what_a_search_compared_with_is_kept_as_before() {
        let mut f = finder(0, &[3, 3]);
        f.narrow(Search::Changed, &[3, 4]);
        assert_eq!(f.before(1), Some(3));
        f.narrow(Search::WentUp, &[3, 6]);
        assert_eq!(f.before(1), Some(4));
        assert_eq!(f.before(9), None);
    }

    #[test]
    fn words_are_read_in_the_machines_order() {
        let mut f = Finder { width: Width::Word, ..Finder::default() };
        f.start(0, &[0x34, 0x12, 0x00]);
        // Two words fit in three bytes.
        assert_eq!(f.count(), 2);
        assert_eq!(f.narrow(Search::Equals(0x1234), &[0x34, 0x12, 0x00]), 1);
        let mut big = Finder { width: Width::Word, big_endian: true, ..Finder::default() };
        big.start(0, &[0x12, 0x34, 0x00]);
        assert_eq!(big.narrow(Search::Equals(0x1234), &[0x12, 0x34, 0x00]), 1);
        assert_eq!(big.candidates(), [0]);
        assert_eq!(bytes_of(0x1234, Width::Word, true), vec![0x12, 0x34]);
        assert_eq!(bytes_of(0x1234, Width::Word, false), vec![0x34, 0x12]);
        assert_eq!(bytes_of(0x1FF, Width::Byte, false), vec![0xFF]);
    }

    #[test]
    fn a_search_before_a_snapshot_or_on_other_memory_changes_nothing() {
        let mut f = Finder::default();
        assert_eq!(f.narrow(Search::Changed, &[1, 2]), 0);
        let mut f = finder(0, &[1, 2, 3]);
        assert_eq!(f.narrow(Search::Changed, &[1, 2]), 3);
        f.set_width(Width::Word);
        assert!(!f.started());
    }

    #[test]
    fn freezes_are_kept_per_program_and_read_back() {
        let dir = std::env::temp_dir().join(format!("limoniter-trainer-test-{}", std::process::id()));
        let path = Kept::file_in(&dir);
        let lives = Freeze { address: 0x5C00, value: 3, width: Width::Byte, name: "Lives".into(), on: true, block: false };
        let score = Freeze { address: 0x5C10, value: 0x1234, width: Width::Word, name: String::new(), on: false, block: true };
        let mut kept = Kept::load(Some(path.clone()));
        kept.set("game.tap", vec![lives.clone(), score.clone()]);
        kept.set("other.tap", vec![lives.clone()]);
        let again = Kept::load(Some(path.clone()));
        assert_eq!(again.of("game.tap"), [lives.clone(), score]);
        assert_eq!(again.of("other.tap"), [lives]);
        assert!(again.of("never.tap").is_empty());
        let mut again = again;
        again.set("other.tap", Vec::new());
        assert!(Kept::load(Some(path)).of("other.tap").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_frozen_addresses_that_are_on_are_written_back() {
        let on = Freeze { address: 0x80, value: 0x1234, width: Width::Word, name: String::new(), on: true, block: false };
        let off = Freeze { address: 0x90, value: 1, width: Width::Byte, name: String::new(), on: false, block: false };
        assert_eq!(writes_due(&[on, off], true), vec![(0x80, vec![0x12, 0x34])]);
    }

    #[test]
    fn numbers_are_typed_in_decimal_or_hex() {
        assert_eq!(parse_number(" 42 "), Some(42));
        assert_eq!(parse_number("$FF"), Some(255));
        assert_eq!(parse_number("0x10"), Some(16));
        assert_eq!(parse_number("lots"), None);
    }
}
