//! Remembers an explicit encoding choice for each file, until it's cleared.

use crate::charset;
use crate::codec::{Certainty, Display, Eol, NewlinePolicy, TextFormat};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct EncodingPrefs {
    entries: BTreeMap<String, String>,
    path: Option<PathBuf>,
}

fn key_of(file: &Path) -> String {
    file.to_string_lossy().to_lowercase()
}

/// charset, line policy, display, starting state, separated by tabs.
fn to_line(format: &TextFormat) -> String {
    format!("{}\t{}\t{}\t{}", format.charset.id, format.newline.key(), format.display.key(), format.state)
}

fn from_line(line: &str) -> Option<TextFormat> {
    let mut parts = line.split('\t');
    let set = charset::by_id(parts.next()?)?;
    let newline = NewlinePolicy::from_key(parts.next()?)?;
    let display = Display::from_key(parts.next()?)?;
    let state = parts.next()?.parse().ok()?;
    let preset_eol = match newline {
        NewlinePolicy::AtasciiEol => Eol::Atascii,
        NewlinePolicy::CrOnly => Eol::Cr,
        NewlinePolicy::FixedRows { .. } => Eol::None,
        NewlinePolicy::Standard if set.ebcdic => Eol::Nl,
        NewlinePolicy::Standard => Eol::CrLf,
    };
    Some(TextFormat { charset: set, newline, display, state, preset_eol, certainty: Certainty::Remembered, ..TextFormat::default() })
}

impl EncodingPrefs {
    /// Reads the list that sits beside the settings. A line that can't be read is ignored.
    pub fn load(path: Option<PathBuf>) -> Self {
        let mut entries = BTreeMap::new();
        if let Some(text) = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
            for line in text.lines() {
                if let Some((file, rest)) = line.split_once('\t') {
                    entries.insert(file.to_string(), rest.to_string());
                }
            }
        }
        Self { entries, path }
    }

    pub fn get(&self, file: &Path) -> Option<TextFormat> {
        self.entries.get(&key_of(file)).and_then(|line| from_line(line))
    }

    pub fn has(&self, file: &Path) -> bool {
        self.entries.contains_key(&key_of(file))
    }

    pub fn set(&mut self, file: &Path, format: &TextFormat) {
        self.entries.insert(key_of(file), to_line(format));
        self.save();
    }

    pub fn clear(&mut self, file: &Path) {
        self.entries.remove(&key_of(file));
        self.save();
    }

    /// Not being able to save a preference shouldn't get in the way of editing, so a failure is ignored.
    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text: String = self.entries.iter().map(|(file, rest)| format!("{file}\t{rest}\n")).collect();
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_is_remembered_until_cleared() {
        let dir = std::env::temp_dir().join(format!("limoniter-prefs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = dir.join("encodings.txt");
        let file = Path::new(r"C:\Some\File.TXT");
        let mut prefs = EncodingPrefs::load(Some(store.clone()));
        assert!(prefs.get(file).is_none());
        let format = crate::codec::preset_by_id("atari-8bit").unwrap().format();
        prefs.set(file, &format);

        let again = EncodingPrefs::load(Some(store.clone()));
        let found = again.get(Path::new(r"c:\some\file.txt")).unwrap();
        assert_eq!(found.charset.id, "atascii");
        assert_eq!(found.newline, NewlinePolicy::AtasciiEol);
        assert_eq!(found.preset_eol, Eol::Atascii);
        assert_eq!(found.certainty, Certainty::Remembered);

        prefs.clear(file);
        assert!(!EncodingPrefs::load(Some(store)).has(file));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lines_that_cant_be_read_are_ignored() {
        assert!(from_line("no-such-set\tstandard\tplain\t0").is_none());
        assert!(from_line("utf-8\tnonsense\tplain\t0").is_none());
        assert!(from_line("utf-8").is_none());
        assert!(from_line("teletext-de\trows-40-parity\tteletext\t0").unwrap().newline.parity());
    }
}
