//! Time spent, kept on this computer and sent nowhere (as WakaTime's plugin keeps it, but local): each minute
//! someone's working in a file counts towards the day, the project, the file and its language. A pause of more
//! than two minutes doesn't count. It's kept in time-spent.tsv beside the settings, a line a day and file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A pause longer than this isn't counted as working.
const IDLE: Duration = Duration::from_secs(120);
/// How often what's been counted is written out.
const SAVE_EVERY: Duration = Duration::from_secs(60);

/// What a stretch of time is counted under.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
    /// The day, as 2026-11-19.
    pub day: String,
    pub project: String,
    pub file: String,
    pub language: String,
}

/// Seconds spent, under each key.
#[derive(Default)]
pub struct Tracker {
    pub totals: BTreeMap<Key, u64>,
    last: Option<Instant>,
    saved: Option<Instant>,
    /// Seconds counted since the last save, not yet written.
    unsaved: bool,
    path: Option<PathBuf>,
}

pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

impl Tracker {
    /// Read from the file it's kept in.
    pub fn load(path: Option<PathBuf>) -> Tracker {
        let mut tracker = Tracker::default();
        tracker.path = path.clone();
        if let Some(text) = path.and_then(|p| std::fs::read_to_string(p).ok()) {
            tracker.totals = parse(&text);
        }
        tracker
    }

    /// Someone did something in this file just now: the time since the last thing counts, if it was short.
    pub fn activity(&mut self, project: &str, file: &str, language: &str, now: Instant) {
        if let Some(last) = self.last {
            let gap = now.saturating_duration_since(last);
            if gap <= IDLE && gap.as_secs() > 0 {
                let key = Key { day: today(), project: project.to_string(), file: file.to_string(), language: language.to_string() };
                *self.totals.entry(key).or_insert(0) += gap.as_secs();
                self.unsaved = true;
                self.last = Some(now);
            } else if gap > IDLE {
                self.last = Some(now);
            }
        } else {
            self.last = Some(now);
        }
        if self.unsaved && self.saved.is_none_or(|s| now.saturating_duration_since(s) >= SAVE_EVERY) {
            self.save();
            self.saved = Some(now);
        }
    }

    pub fn save(&mut self) {
        if let Some(path) = &self.path {
            if let Some(folder) = path.parent() {
                let _ = std::fs::create_dir_all(folder);
            }
            let _ = std::fs::write(path, to_text(&self.totals));
            self.unsaved = false;
        }
    }

    /// The totals for the days from `from` on (as 2026-11-19), by what `by` picks out of each key, largest first.
    pub fn summary(&self, from: &str, by: impl Fn(&Key) -> String) -> Vec<(String, u64)> {
        let mut sums: BTreeMap<String, u64> = BTreeMap::new();
        for (key, seconds) in self.totals.iter().filter(|(k, _)| k.day.as_str() >= from) {
            *sums.entry(by(key)).or_insert(0) += seconds;
        }
        let mut list: Vec<(String, u64)> = sums.into_iter().collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        list
    }
}

/// What's been counted since the last save isn't lost when Limoniter closes.
impl Drop for Tracker {
    fn drop(&mut self) {
        if self.unsaved {
            self.save();
        }
    }
}

fn parse(text: &str) -> BTreeMap<Key, u64> {
    let mut totals = BTreeMap::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if let [day, project, file, language, seconds] = parts[..]
            && let Ok(seconds) = seconds.parse::<u64>()
        {
            *totals.entry(Key { day: day.into(), project: project.into(), file: file.into(), language: language.into() }).or_insert(0) += seconds;
        }
    }
    totals
}

fn to_text(totals: &BTreeMap<Key, u64>) -> String {
    totals.iter().map(|(k, s)| format!("{}\t{}\t{}\t{}\t{s}\n", k.day, k.project.replace('\t', " "), k.file.replace('\t', " "), k.language)).collect()
}

/// Seconds as hours and minutes: 2 h 05 min, or 12 min.
pub fn shown(seconds: u64) -> String {
    let minutes = seconds / 60;
    if minutes >= 60 { format!("{} h {:02} min", minutes / 60, minutes % 60) } else { format!("{minutes} min") }
}

/// The day a number of days before today.
pub fn days_ago(days: i64) -> String {
    (chrono::Local::now() - chrono::Duration::days(days)).format("%Y-%m-%d").to_string()
}

/// A file's name as it's kept: relative to its project's folder if it has one.
pub fn file_label(file: &Path, project_folder: Option<&Path>) -> String {
    project_folder.and_then(|f| file.strip_prefix(f).ok()).unwrap_or(file).display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_time_counts_and_pauses_dont() {
        let path = std::env::temp_dir().join(format!("limoniter-time-{}.tsv", std::process::id()));
        let mut tracker = Tracker::load(Some(path.clone()));
        let start = Instant::now();
        tracker.activity("game", "main.s", "Assembly 6502", start);
        tracker.activity("game", "main.s", "Assembly 6502", start + Duration::from_secs(30));
        tracker.activity("game", "main.s", "Assembly 6502", start + Duration::from_secs(90));
        // A long pause, then more work: the pause isn't counted.
        tracker.activity("game", "gfx.s", "Assembly 6502", start + Duration::from_secs(1000));
        tracker.activity("game", "gfx.s", "Assembly 6502", start + Duration::from_secs(1060));
        let by_file = tracker.summary(&today(), |k| k.file.clone());
        assert_eq!(by_file, [("main.s".to_string(), 90), ("gfx.s".to_string(), 60)]);
        tracker.save();
        let again = Tracker::load(Some(path.clone()));
        assert_eq!(again.summary("0000", |k| k.language.clone()), [("Assembly 6502".to_string(), 150)]);
        assert_eq!(shown(3900), "1 h 05 min");
        assert_eq!(shown(720), "12 min");
        let _ = std::fs::remove_file(&path);
    }
}
