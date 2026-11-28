//! What the two tools have used, added up from the logs they keep themselves, so that sessions run in a terminal
//! count as well as those in Limoniter's own panel: Claude Code's `projects/*/*.jsonl` (in its settings folder) and
//! Codex's `sessions/YYYY/MM/DD/rollout-*.jsonl`. Each file is read once and then only what has been added to it,
//! and a last line that is still being written is left until it is whole.
//!
//! Both logs are the tools' own and not promised to stay as they are: a line that can't be understood is skipped.

use crate::agent_session::{Limit, Limits};
use chrono::{NaiveDate, TimeZone};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Which tool a log belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Which {
    Claude,
    Codex,
}

impl Which {
    pub fn name(self) -> &'static str {
        match self {
            Which::Claude => "Claude Code",
            Which::Codex => "Codex",
        }
    }
}

/// Tokens, in the four kinds both tools can be made to say. Input here is only what wasn't read from the cache,
/// so the kinds never overlap and can simply be added up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Counts {
    pub fn add(&mut self, other: Counts) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
    }

    fn take(&mut self, other: Counts) {
        self.input = self.input.saturating_sub(other.input);
        self.output = self.output.saturating_sub(other.output);
        self.cache_read = self.cache_read.saturating_sub(other.cache_read);
        self.cache_write = self.cache_write.saturating_sub(other.cache_write);
    }

    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }

    fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

/// Where a tool keeps its logs.
#[derive(Clone, Debug, PartialEq)]
pub struct Root {
    pub which: Which,
    /// Claude's settings folder, or Codex's home.
    pub dir: PathBuf,
}

/// How long a day's range is looked at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    Today,
    Week,
}

impl Range {
    /// The first day in the range, given today.
    pub fn first_day(self, today: NaiveDate) -> NaiveDate {
        match self {
            Range::Today => today,
            Range::Week => today - chrono::Days::new(6),
        }
    }
}

/// A session whose log has been written to lately.
#[derive(Clone, Debug, PartialEq)]
pub struct Running {
    pub which: Which,
    pub id: String,
    pub folder: Option<String>,
    pub model: Option<String>,
    /// Everything it has used.
    pub counts: Counts,
    /// The size of its last prompt, and the window it has, where the log says (Codex does; Claude's doesn't).
    pub context: Option<(u64, Option<u64>)>,
    /// When its log was last written, as seconds since 1970.
    pub written: i64,
    /// Where the log is (not shown: it tells the panel which of its own sessions this is).
    pub path: PathBuf,
}

/// One log file as far as it has been read.
#[derive(Default)]
struct FileState {
    offset: u64,
    /// Codex's running total, to take what each report adds.
    codex_total: Counts,
    model: Option<String>,
    folder: Option<String>,
    counts: Counts,
    context: Option<(u64, Option<u64>)>,
}

#[derive(Clone)]
struct Entry {
    day: NaiveDate,
    model: String,
    counts: Counts,
}

/// Everything read so far.
pub struct Scanner {
    files: HashMap<PathBuf, FileState>,
    /// Claude writes a reply's usage on each line of the reply: it's counted once, by the reply's name.
    seen: HashMap<String, Entry>,
    totals: BTreeMap<(Which, String, NaiveDate), Counts>,
    /// The newest rate limits Codex's logs mention, and when.
    pub codex_limits: Option<(i64, Limits)>,
    pub running: Vec<Running>,
    day_of: Box<dyn Fn(i64) -> NaiveDate + Send>,
}

/// How recently a log has to have been written for its session to count as running.
const RUNNING_WITHIN: i64 = 5 * 60;
/// Logs older than this many days (plus a day's slack) hold nothing a week's total needs.
const KEEP_DAYS: u64 = 8;

/// The day a moment falls in by the computer's own clock.
pub fn local_day(epoch: i64) -> NaiveDate {
    chrono::Local.timestamp_opt(epoch, 0).single().map_or(NaiveDate::default(), |t| t.date_naive())
}

impl Default for Scanner {
    fn default() -> Self {
        Scanner::with_day(Box::new(local_day))
    }
}

impl Scanner {
    /// A scanner that tells days by the function given: tests give it a fixed offset from UTC.
    pub fn with_day(day_of: Box<dyn Fn(i64) -> NaiveDate + Send>) -> Self {
        Scanner { files: HashMap::new(), seen: HashMap::new(), totals: BTreeMap::new(), codex_limits: None, running: Vec::new(), day_of }
    }

    /// Reads what has been added to every log in the roots since the last time.
    pub fn scan(&mut self, roots: &[Root], now: i64) {
        let today = (self.day_of)(now);
        let oldest = Range::Week.first_day(today);
        let mut running = Vec::new();
        let mut present = HashSet::new();
        for root in roots {
            for (path, top_level) in log_files(root, now) {
                present.insert(path.clone());
                let written = modified(&path);
                self.read_file(root.which, &path, oldest);
                if top_level
                    && now - written <= RUNNING_WITHIN
                    && let Some(state) = self.files.get(&path)
                {
                    let id = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    running.push(Running { which: root.which, id, folder: state.folder.clone(), model: state.model.clone(), counts: state.counts, context: state.context, written, path: path.clone() });
                }
            }
        }
        running.sort_by_key(|r| std::cmp::Reverse(r.written));
        self.running = running;
        // What fell out of the week, and files that have gone, needn't be kept.
        self.totals.retain(|(_, _, day), _| *day >= oldest);
        self.seen.retain(|_, entry| entry.day >= oldest);
        self.files.retain(|path, _| present.contains(path));
    }

    fn read_file(&mut self, which: Which, path: &Path, oldest: NaiveDate) {
        let Ok(mut file) = std::fs::File::open(path) else { return };
        let length = file.metadata().map_or(0, |m| m.len());
        let mut state = self.files.remove(path).unwrap_or_default();
        if length < state.offset {
            // Shortened or replaced: start again. A reply already counted is replaced by its new count.
            state.offset = 0;
        }
        if length > state.offset && file.seek(SeekFrom::Start(state.offset)).is_ok() {
            let mut bytes = Vec::new();
            if file.take(length - state.offset).read_to_end(&mut bytes).is_ok() {
                // Only whole lines: one still being written waits for the rest of it.
                let whole = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |at| at + 1);
                for line in bytes[..whole].split(|b| *b == b'\n') {
                    if let Ok(value) = serde_json::from_slice::<Value>(line) {
                        match which {
                            Which::Claude => self.claude_line(&value, &mut state, oldest),
                            Which::Codex => self.codex_line(&value, &mut state, oldest),
                        }
                    }
                }
                state.offset += whole as u64;
            }
        }
        self.files.insert(path.to_path_buf(), state);
    }

    fn put(&mut self, which: Which, model: &str, day: NaiveDate, counts: Counts) {
        if !counts.is_empty() {
            self.totals.entry((which, model.to_string(), day)).or_default().add(counts);
        }
    }

    fn claude_line(&mut self, line: &Value, state: &mut FileState, oldest: NaiveDate) {
        if state.folder.is_none() {
            state.folder = line.get("cwd").and_then(Value::as_str).map(str::to_string);
        }
        if line.get("type").and_then(Value::as_str) != Some("assistant") {
            return;
        }
        let Some(message) = line.get("message") else { return };
        let Some(usage) = message.get("usage") else { return };
        let model = message.get("model").and_then(Value::as_str).unwrap_or("unknown");
        if model.starts_with('<') {
            return;
        }
        let Some(at) = line.get("timestamp").and_then(Value::as_str).and_then(parse_time) else { return };
        let number = |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
        let counts = Counts { input: number("input_tokens"), output: number("output_tokens"), cache_read: number("cache_read_input_tokens"), cache_write: number("cache_creation_input_tokens") };
        state.model = Some(model.to_string());
        let key = message.get("id").or_else(|| line.get("requestId")).or_else(|| line.get("uuid")).and_then(Value::as_str).map(str::to_string);
        // The size of the prompt of the latest reply is how full the context is.
        state.context = Some((counts.input + counts.cache_read + counts.cache_write, None));
        let day = (self.day_of)(at);
        match key {
            Some(key) => {
                // A reply seen again (a line for each block of it, or a log read again) replaces what was counted.
                let earlier = self.seen.get(&key).cloned();
                if let Some(earlier) = &earlier {
                    if let Some(total) = self.totals.get_mut(&(Which::Claude, earlier.model.clone(), earlier.day)) {
                        total.take(earlier.counts);
                    }
                    state.counts.take(earlier.counts);
                }
                state.counts.add(counts);
                if day >= oldest {
                    self.put(Which::Claude, model, day, counts);
                    self.seen.insert(key, Entry { day, model: model.to_string(), counts });
                }
            }
            None => {
                state.counts.add(counts);
                if day >= oldest {
                    self.put(Which::Claude, model, day, counts);
                }
            }
        }
    }

    fn codex_line(&mut self, line: &Value, state: &mut FileState, oldest: NaiveDate) {
        let payload = line.get("payload");
        match line.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                state.folder = payload.and_then(|p| p.get("cwd")).and_then(Value::as_str).map(str::to_string).or(state.folder.take());
            }
            Some("turn_context") => {
                if let Some(model) = payload.and_then(|p| p.get("model")).and_then(Value::as_str) {
                    state.model = Some(model.to_string());
                }
            }
            Some("event_msg") => {
                let Some(payload) = payload.filter(|p| p.get("type").and_then(Value::as_str) == Some("token_count")) else { return };
                let Some(at) = line.get("timestamp").and_then(Value::as_str).and_then(parse_time) else { return };
                if let Some(info) = payload.get("info").filter(|i| i.is_object()) {
                    self.codex_tokens(info, at, state, oldest);
                }
                if let Some(limits) = payload.get("rate_limits").and_then(codex_limits)
                    && self.codex_limits.as_ref().is_none_or(|(when, _)| at >= *when)
                {
                    self.codex_limits = Some((at, limits));
                }
            }
            _ => {}
        }
    }

    fn codex_tokens(&mut self, info: &Value, at: i64, state: &mut FileState, oldest: NaiveDate) {
        if let Some(window) = info.get("model_context_window").and_then(Value::as_u64) {
            let used = info.get("last_token_usage").and_then(|u| u.get("input_tokens")).and_then(Value::as_u64).unwrap_or(0);
            state.context = Some((used, Some(window)));
        }
        let Some(total) = info.get("total_token_usage") else { return };
        let number = |name: &str| total.get(name).and_then(Value::as_u64).unwrap_or(0);
        // Codex counts the cached part as part of its input; here it's counted apart.
        let (input, cached, output, written) = (number("input_tokens"), number("cached_input_tokens"), number("output_tokens"), number("cache_write_input_tokens"));
        let before = state.codex_total;
        let before_all_input = before.input + before.cache_read;
        let (all_input, now_total) = (input, Counts { input: input.saturating_sub(cached), output, cache_read: cached, cache_write: written });
        // The same report again, or an older one, adds nothing.
        if all_input < before_all_input || output < before.output {
            return;
        }
        let delta = Counts {
            input: now_total.input.saturating_sub(before.input),
            output: now_total.output - before.output,
            cache_read: now_total.cache_read.saturating_sub(before.cache_read),
            cache_write: now_total.cache_write.saturating_sub(before.cache_write),
        };
        state.codex_total = now_total;
        state.counts.add(delta);
        let day = (self.day_of)(at);
        if day >= oldest {
            let model = state.model.clone().unwrap_or_else(|| "unknown".into());
            self.put(Which::Codex, &model, day, delta);
        }
    }

    /// What was used in the range, by tool and model, the most first.
    pub fn totals(&self, range: Range, now: i64) -> Vec<(Which, String, Counts)> {
        let first = range.first_day((self.day_of)(now));
        let mut by_model: BTreeMap<(Which, String), Counts> = BTreeMap::new();
        for ((which, model, day), counts) in &self.totals {
            if *day >= first {
                by_model.entry((*which, model.clone())).or_default().add(*counts);
            }
        }
        let mut rows: Vec<_> = by_model.into_iter().map(|((which, model), counts)| (which, model, counts)).collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0).then(b.2.total().cmp(&a.2.total())));
        rows
    }
}

fn parse_time(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text).ok().map(|t| t.timestamp())
}

/// Codex's limits, from a report's `rate_limits`: the window of about five hours and the one of about a week.
fn codex_limits(value: &Value) -> Option<Limits> {
    if value.get("limit_id").and_then(Value::as_str).is_some_and(|id| id != "codex") {
        return None;
    }
    let mut limits = Limits::default();
    for name in ["primary", "secondary"] {
        let Some(window) = value.get(name).filter(|w| w.is_object()) else { continue };
        let percent = window.get("used_percent").and_then(Value::as_f64)? as f32;
        let minutes = window.get("window_minutes").and_then(Value::as_u64).unwrap_or(0);
        let limit = Limit { percent, resets: window.get("resets_at").and_then(Value::as_i64) };
        if minutes > 0 && minutes <= 12 * 60 {
            limits.five_hour = Some(limit);
        } else if minutes >= 5 * 24 * 60 {
            limits.week = Some(limit);
        }
    }
    limits.any().then_some(limits)
}

fn modified(path: &Path) -> i64 {
    std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64)
}

/// The logs in a root worth reading (written within the last week or so), with whether each is a session's own
/// (a helper's log inside a Claude session's folder is read, but isn't a session of its own).
fn log_files(root: &Root, now: i64) -> Vec<(PathBuf, bool)> {
    let recent = |path: &Path| now - modified(path) <= (KEEP_DAYS * 86400) as i64;
    let mut found = Vec::new();
    let sub = |dir: &Path| std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>();
    match root.which {
        Which::Claude => {
            for project in sub(&root.dir.join("projects")) {
                for entry in sub(&project) {
                    if entry.extension().is_some_and(|e| e == "jsonl") {
                        if recent(&entry) {
                            found.push((entry, true));
                        }
                    } else if entry.is_dir() {
                        for helper in sub(&entry.join("subagents")) {
                            if helper.extension().is_some_and(|e| e == "jsonl") && recent(&helper) {
                                found.push((helper, false));
                            }
                        }
                    }
                }
            }
        }
        Which::Codex => {
            for year in sub(&root.dir.join("sessions")) {
                for month in sub(&year) {
                    for day in sub(&month) {
                        for file in sub(&day) {
                            let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            if name.starts_with("rollout-") && name.ends_with(".jsonl") && recent(&file) {
                                found.push((file, true));
                            }
                        }
                    }
                }
            }
        }
    }
    found.sort();
    found
}

// ---------- Where the logs are ----------

/// Claude's settings folder: CLAUDE_CONFIG_DIR, or `.claude` in the home folder.
pub fn claude_dir() -> Option<PathBuf> {
    crate::claude_ide::config_dir()
}

/// Codex's home: CODEX_HOME, or `.codex` in the home folder.
pub fn codex_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CODEX_HOME").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).filter(|home| !home.is_empty())?;
    Some(PathBuf::from(home).join(".codex"))
}

/// The homes of the people in a WSL distribution, as Windows sees them: `\\wsl.localhost\<name>\home\<person>` and
/// root's. Only those that exist are named.
pub fn wsl_homes(distro_root: &Path) -> Vec<PathBuf> {
    let mut homes: Vec<PathBuf> = std::fs::read_dir(distro_root.join("home")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    let root = distro_root.join("root");
    if root.is_dir() {
        homes.push(root);
    }
    homes.sort();
    homes
}

/// Every place either tool's logs may be: this computer's, and on Windows those in the WSL distributions' homes,
/// where the tools often live.
pub fn roots() -> Vec<Root> {
    let mut roots = Vec::new();
    roots.extend(claude_dir().map(|dir| Root { which: Which::Claude, dir }));
    roots.extend(codex_dir().map(|dir| Root { which: Which::Codex, dir }));
    if cfg!(windows) {
        for shell in crate::terminal::shells() {
            if let crate::terminal::Shell::Wsl(name) = shell {
                for home in wsl_homes(&PathBuf::from(format!("\\\\wsl.localhost\\{name}"))) {
                    for (which, folder) in [(Which::Claude, ".claude"), (Which::Codex, ".codex")] {
                        let dir = home.join(folder);
                        if dir.is_dir() {
                            roots.push(Root { which, dir });
                        }
                    }
                }
            }
        }
    }
    roots.dedup();
    roots
}

// ---------- Which figure is the newest ----------

/// Where a figure came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A session running in Limoniter's panel.
    Panel,
    /// What Claude passes its status line command.
    StatusLine,
    /// A report in a tool's log.
    Log,
}

/// Limits as one place has them, and when it said so (seconds since 1970).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seen {
    pub limits: Limits,
    pub at: i64,
    pub from: Source,
}

/// A limit chosen, with how old the figure is and where it came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fresh {
    pub limit: Limit,
    pub at: i64,
    pub from: Source,
}

impl Fresh {
    /// A window that has ended since the figure was written: what it said no longer holds.
    pub fn has_reset(&self, now: i64) -> bool {
        self.limit.resets.is_some_and(|r| r <= now)
    }
}

/// For each of the two limits, the newest figure from any place. A figure whose window has since ended loses to
/// any whose window hasn't.
pub fn freshest(seen: &[Seen], now: i64) -> (Option<Fresh>, Option<Fresh>) {
    let pick = |get: fn(&Limits) -> Option<Limit>| {
        seen.iter()
            .filter_map(|s| get(&s.limits).map(|limit| Fresh { limit, at: s.at, from: s.from }))
            .max_by_key(|f| (!f.has_reset(now), f.at))
    };
    (pick(|l| l.five_hour), pick(|l| l.week))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    use std::io::Write;

    /// A scanner that tells days at an hour from UTC, as a computer elsewhere would.
    fn scanner_at(hours: i32) -> Scanner {
        let zone = FixedOffset::east_opt(hours * 3600).unwrap();
        Scanner::with_day(Box::new(move |epoch| zone.timestamp_opt(epoch, 0).unwrap().date_naive()))
    }

    fn epoch(text: &str) -> i64 {
        parse_time(text).unwrap()
    }

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-usage-logs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn claude_line(id: &str, time: &str, model: &str, input: u64, output: u64, cache_read: u64, cache_write: u64) -> String {
        format!(r#"{{"type":"assistant","cwd":"/work/app","timestamp":"{time}","message":{{"id":"{id}","model":"{model}","usage":{{"input_tokens":{input},"output_tokens":{output},"cache_read_input_tokens":{cache_read},"cache_creation_input_tokens":{cache_write}}}}}}}"#) + "\n"
    }

    fn claude_root(name: &str, lines: &str) -> (Root, PathBuf) {
        let dir = folder(name);
        let project = dir.join("projects").join("-work-app");
        std::fs::create_dir_all(&project).unwrap();
        let log = project.join("s1.jsonl");
        std::fs::write(&log, lines).unwrap();
        (Root { which: Which::Claude, dir }, log)
    }

    fn codex_report(time: &str, input: u64, cached: u64, output: u64, primary: Option<(f64, u64, i64)>) -> String {
        let limits = match primary {
            Some((used, minutes, resets)) => format!(r#","rate_limits":{{"limit_id":"codex","primary":{{"used_percent":{used},"window_minutes":{minutes},"resets_at":{resets}}},"secondary":{{"used_percent":70.0,"window_minutes":10080,"resets_at":{}}}}}"#, resets + 100_000),
            None => String::new(),
        };
        format!(r#"{{"timestamp":"{time}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"output_tokens":{output}}},"last_token_usage":{{"input_tokens":500}},"model_context_window":258400}}{limits}}}}}"#) + "\n"
    }

    #[test]
    fn claude_lines_are_counted_once_per_reply_by_model() {
        // The reply m1 is written on two lines (a thinking block, a tool use); a malformed line sits between.
        let text = [
            claude_line("m1", "2026-11-27T10:00:00Z", "claude-opus-5-5", 2, 165, 23410, 15032),
            "{this is not json\n".to_string(),
            claude_line("m1", "2026-11-27T10:00:01Z", "claude-opus-5-5", 2, 165, 23410, 15032),
            claude_line("m2", "2026-11-27T10:05:00Z", "claude-haiku-4-5", 10, 20, 30, 40),
            r#"{"type":"user","message":{"content":"hello"}}"#.to_string() + "\n",
        ]
        .concat();
        let (root, _) = claude_root("claude-once", &text);
        let mut scanner = scanner_at(0);
        let now = epoch("2026-11-27T12:00:00Z");
        scanner.scan(&[root], now);
        let rows = scanner.totals(Range::Today, now);
        assert_eq!(rows.len(), 2);
        let opus = rows.iter().find(|r| r.1 == "claude-opus-5-5").unwrap();
        assert_eq!(opus.2, Counts { input: 2, output: 165, cache_read: 23410, cache_write: 15032 });
        let haiku = rows.iter().find(|r| r.1 == "claude-haiku-4-5").unwrap();
        assert_eq!(haiku.2, Counts { input: 10, output: 20, cache_read: 30, cache_write: 40 });
    }

    #[test]
    fn a_partial_last_line_waits_and_only_new_lines_are_read() {
        let first = claude_line("m1", "2026-11-27T10:00:00Z", "claude-opus-5-5", 1, 2, 3, 4);
        let second = claude_line("m2", "2026-11-27T10:01:00Z", "claude-opus-5-5", 10, 20, 30, 40);
        let (cut, rest) = second.split_at(second.len() / 2);
        let (root, log) = claude_root("partial", &format!("{first}{cut}"));
        let mut scanner = scanner_at(0);
        let now = epoch("2026-11-27T12:00:00Z");
        scanner.scan(std::slice::from_ref(&root), now);
        assert_eq!(scanner.totals(Range::Today, now)[0].2.output, 2, "the half line isn't read");
        // The rest of it arrives, and another whole line; the first isn't counted twice.
        let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        file.write_all(rest.as_bytes()).unwrap();
        file.write_all(claude_line("m3", "2026-11-27T10:02:00Z", "claude-opus-5-5", 100, 200, 300, 400).as_bytes()).unwrap();
        drop(file);
        scanner.scan(&[root], now);
        let counts = scanner.totals(Range::Today, now)[0].2;
        assert_eq!(counts, Counts { input: 111, output: 222, cache_read: 333, cache_write: 444 });
    }

    #[test]
    fn today_and_week_follow_local_midnight() {
        // 23:30 UTC on the 26th is 01:30 on the 27th at two hours east of Greenwich, and still the 26th in Greenwich.
        let text = [
            claude_line("a", "2026-11-26T23:30:00Z", "m", 0, 100, 0, 0),
            claude_line("b", "2026-11-27T09:00:00Z", "m", 0, 10, 0, 0),
            claude_line("c", "2026-11-20T12:00:00Z", "m", 0, 1000, 0, 0),
            claude_line("d", "2026-11-21T12:00:00Z", "m", 0, 5, 0, 0),
        ]
        .concat();
        let now = epoch("2026-11-27T12:00:00Z");
        let (root, _) = claude_root("midnight", &text);
        let output = |scanner: &Scanner, range| scanner.totals(range, now).iter().map(|r| r.2.output).sum::<u64>();
        let mut greenwich = scanner_at(0);
        greenwich.scan(std::slice::from_ref(&root), now);
        assert_eq!(output(&greenwich, Range::Today), 10);
        // The week is today and the six days before: the 21st is in, the 20th is out.
        assert_eq!(output(&greenwich, Range::Week), 100 + 10 + 5);
        let mut east = scanner_at(2);
        east.scan(&[root], now);
        assert_eq!(output(&east, Range::Today), 110, "the 23:30 line is after midnight there");
    }

    #[test]
    fn codex_totals_become_what_each_report_adds() {
        let lines = [
            r#"{"timestamp":"2026-11-27T09:00:00Z","type":"session_meta","payload":{"cwd":"/work/app"}}"#.to_string() + "\n",
            r#"{"timestamp":"2026-11-27T09:00:01Z","type":"turn_context","payload":{"model":"gpt-5.5"}}"#.to_string() + "\n",
            codex_report("2026-11-26T23:00:00Z", 1000, 800, 50, None),
            // Reported twice, as Codex does, and then grown.
            codex_report("2026-11-27T09:00:05Z", 3000, 2000, 80, Some((42.0, 300, 1_795_620_498))),
            codex_report("2026-11-27T09:00:05Z", 3000, 2000, 80, None),
            "garbage\n".to_string(),
            codex_report("2026-11-27T09:10:00Z", 4000, 2900, 100, Some((43.0, 300, 1_795_620_498))),
        ]
        .concat();
        let dir = folder("codex");
        let day = dir.join("sessions/2026/11/27");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(day.join("rollout-2026-11-27T09-00-00-abc.jsonl"), lines).unwrap();
        let root = Root { which: Which::Codex, dir };
        let now = epoch("2026-11-27T12:00:00Z");
        let mut scanner = scanner_at(0);
        scanner.scan(&[root], now);
        let today = scanner.totals(Range::Today, now);
        assert_eq!(today.len(), 1);
        assert_eq!(today[0].1, "gpt-5.5");
        // 4000 in all, less the 1000 of yesterday: of the 3000 more, 2100 came from the cache.
        assert_eq!(today[0].2, Counts { input: 900, output: 50, cache_read: 2100, cache_write: 0 });
        let week = scanner.totals(Range::Week, now);
        assert_eq!(week[0].2.output, 100);
        assert_eq!(week[0].2.input + week[0].2.cache_read, 4000);
        // The newest report's limits, with the windows told apart by their length.
        let (at, limits) = scanner.codex_limits.unwrap();
        assert_eq!(at, epoch("2026-11-27T09:10:00Z"));
        assert_eq!(limits.five_hour.unwrap().percent, 43.0);
        assert_eq!(limits.week.unwrap().percent, 70.0);
        // Its log was just written (by the test), so it's running, with its last prompt and window.
        let mut live = scanner_at(0);
        live.scan(&[Root { which: Which::Codex, dir: scanner.files.keys().next().unwrap().ancestors().nth(5).unwrap().to_path_buf() }], modified(scanner.files.keys().next().unwrap()));
        assert_eq!(live.running.len(), 1);
        assert_eq!(live.running[0].context, Some((500, Some(258_400))));
        assert_eq!(live.running[0].folder.as_deref(), Some("/work/app"));
    }

    #[test]
    fn a_claude_session_running_is_listed_with_its_tokens_and_helpers_add_to_the_total_only() {
        let (root, log) = claude_root("running", &claude_line("m1", "2026-11-27T10:00:00Z", "claude-opus-5-5", 5, 6, 700, 80));
        let helpers = log.with_extension("").join("subagents");
        std::fs::create_dir_all(&helpers).unwrap();
        std::fs::write(helpers.join("agent-1.jsonl"), claude_line("h1", "2026-11-27T10:01:00Z", "claude-haiku-4-5", 1, 1, 1, 1)).unwrap();
        let mut scanner = scanner_at(0);
        scanner.scan(&[root], modified(&log));
        assert_eq!(scanner.running.len(), 1);
        let session = &scanner.running[0];
        assert_eq!((session.id.as_str(), session.folder.as_deref(), session.counts.output), ("s1", Some("/work/app"), 6));
        assert_eq!(session.context, Some((785, None)));
        let now = epoch("2026-11-27T12:00:00Z");
        assert_eq!(scanner.totals(Range::Today, now).len(), 2, "the helper's own model is counted");
    }

    #[test]
    fn freshest_takes_the_newest_figure_for_each_limit() {
        let limit = |percent: f32, resets: i64| Some(Limit { percent, resets: Some(resets) });
        let now = 1_000_000;
        let old = Seen { limits: Limits { five_hour: limit(10.0, now + 100), week: limit(20.0, now + 5000) }, at: now - 600, from: Source::StatusLine };
        let new = Seen { limits: Limits { five_hour: limit(30.0, now + 200), week: None }, at: now - 10, from: Source::Panel };
        let (five, week) = freshest(&[old, new], now);
        assert_eq!((five.unwrap().limit.percent, five.unwrap().from), (30.0, Source::Panel));
        assert_eq!((week.unwrap().limit.percent, week.unwrap().from), (20.0, Source::StatusLine), "the newer one says nothing of the week");
        // A newer figure for a window that has since ended loses to an older one that still holds.
        let ended = Seen { limits: Limits { five_hour: limit(99.0, now - 1), week: None }, at: now - 5, from: Source::Log };
        let (five, _) = freshest(&[old, ended], now);
        assert_eq!(five.unwrap().limit.percent, 10.0);
        // With nothing else it is still given, marked as ended.
        let (five, _) = freshest(&[ended], now);
        assert!(five.unwrap().has_reset(now));
        assert_eq!(freshest(&[], now), (None, None));
    }

    #[test]
    fn wsl_homes_are_found_under_the_distribution() {
        let dir = folder("wsl");
        std::fs::create_dir_all(dir.join("home/ann/.claude")).unwrap();
        std::fs::create_dir_all(dir.join("home/bob")).unwrap();
        std::fs::create_dir_all(dir.join("root")).unwrap();
        std::fs::write(dir.join("home/not-a-home"), "").unwrap();
        let homes = wsl_homes(&dir);
        assert_eq!(homes, vec![dir.join("home/ann"), dir.join("home/bob"), dir.join("root")]);
    }
}
