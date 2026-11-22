//! What the data viewer works out about a table, with nothing drawn: guessing what each column holds, putting
//! the rows in order and finding the ones that match what's typed, summing a column up, and turning cells into
//! text to copy or save. The window that shows it is in `data_view_host`.

use crate::data_table::{Cell, ColumnKind, DataTable};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

/// The year, month and day of a day counted from 1970-01-01 (Howard Hinnant's civil calendar arithmetic).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((year_of_era + era * 400) + i64::from(month <= 2), month, day)
}

/// The day, counted from 1970-01-01, of a year, month and day, if they make a real date.
pub fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || day == 0 {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let longest = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day > longest {
        return None;
    }
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let year_of_era = y.rem_euclid(400);
    let mp = i64::from(if month > 2 { month - 3 } else { month + 9 });
    let day_of_year = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

/// A date written 2026-11-22 (or 2026/11/22), as its day number.
pub fn parse_date(text: &str) -> Option<i64> {
    let text = text.trim();
    let mut parts = text.splitn(3, ['-', '/']);
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if y.len() != 4 || m.len() > 2 || d.len() != 2 || !(y.bytes().chain(m.bytes()).chain(d.bytes())).all(|b| b.is_ascii_digit()) {
        return None;
    }
    days_from_civil(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?)
}

/// A date and time written 2026-11-22 14:05:00 (or with a T, fractions of a second, and Z or an offset), as
/// seconds from 1970-01-01 00:00 UTC.
pub fn parse_date_time(text: &str) -> Option<f64> {
    let text = text.trim();
    let split = text.find(['T', ' '])?;
    let days = parse_date(&text[..split])?;
    let rest = &text[split + 1..];
    // The offset, if there is one: Z, +01:00, -0500.
    let (clock, offset) = match rest.find(['Z', 'z', '+']).or_else(|| rest.rfind('-').filter(|&i| i >= 5)) {
        Some(i) => {
            let tail = &rest[i..];
            let seconds = if tail.eq_ignore_ascii_case("z") {
                0
            } else {
                let digits: String = tail[1..].chars().filter(char::is_ascii_digit).collect();
                if digits.len() != 4 && digits.len() != 2 || tail[1..].chars().any(|c| !c.is_ascii_digit() && c != ':') {
                    return None;
                }
                let hours: i64 = digits[..2].parse().ok()?;
                let minutes: i64 = if digits.len() == 4 { digits[2..].parse().ok()? } else { 0 };
                let sign = if tail.starts_with('-') { -1 } else { 1 };
                sign * (hours * 3600 + minutes * 60)
            };
            (&rest[..i], seconds)
        }
        None => (rest, 0),
    };
    let mut fields = clock.splitn(3, ':');
    let hour: i64 = fields.next()?.parse().ok()?;
    let minute: i64 = fields.next()?.parse().ok()?;
    let second: f64 = fields.next().map_or(Some(0.0), |s| s.parse().ok())?;
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) || !(0.0..61.0).contains(&second) {
        return None;
    }
    Some((days * 86_400 + hour * 3600 + minute * 60 - offset) as f64 + second)
}

/// What a column of cells seems to hold. Real values are told apart from whole numbers, dates from date-times,
/// and anything mixed is text. A column with nothing in it is text.
pub fn guess_kind<'a>(cells: impl Iterator<Item = &'a Cell>) -> ColumnKind {
    let (mut integers, mut reals, mut bools, mut dates, mut date_times, mut others, mut jsons, mut bytes) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut total = 0usize;
    for cell in cells {
        // A big column is judged by its first 20,000 filled cells.
        if total >= 20_000 {
            break;
        }
        match cell {
            Cell::Null => continue,
            Cell::Int(_) => integers += 1,
            Cell::Real(_) => reals += 1,
            Cell::Bool(_) => bools += 1,
            Cell::Bytes(_) => bytes += 1,
            Cell::Text(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let with_leading_zero = trimmed.len() > 1 && trimmed.starts_with('0') && trimmed.bytes().all(|b| b.is_ascii_digit());
                if with_leading_zero {
                    others += 1;
                } else if trimmed.parse::<i64>().is_ok() && !trimmed.starts_with('+') {
                    integers += 1;
                } else if trimmed.parse::<f64>().is_ok_and(f64::is_finite) && !trimmed.eq_ignore_ascii_case("nan") && !trimmed.to_ascii_lowercase().contains("inf") {
                    reals += 1;
                } else if trimmed.eq_ignore_ascii_case("true") || trimmed.eq_ignore_ascii_case("false") {
                    bools += 1;
                } else if parse_date(trimmed).is_some() {
                    dates += 1;
                } else if parse_date_time(trimmed).is_some() {
                    date_times += 1;
                } else if (trimmed.starts_with('{') && trimmed.ends_with('}') || trimmed.starts_with('[') && trimmed.ends_with(']')) && serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
                    jsons += 1;
                } else {
                    others += 1;
                }
            }
        }
        total += 1;
    }
    if total == 0 {
        return ColumnKind::Text;
    }
    let only = |n: usize| n == total;
    if only(integers) {
        ColumnKind::Integer
    } else if only(integers + reals) {
        ColumnKind::Real
    } else if only(bools) {
        ColumnKind::Bool
    } else if only(dates) {
        ColumnKind::Date
    } else if only(dates + date_times) {
        ColumnKind::DateTime
    } else if only(jsons) {
        ColumnKind::Json
    } else if only(bytes) {
        ColumnKind::Binary
    } else {
        let _ = others;
        ColumnKind::Text
    }
}

/// The table with every `Unknown` column's kind guessed from its values.
pub fn with_kinds_guessed(mut table: DataTable) -> DataTable {
    for index in 0..table.columns.len() {
        if table.columns[index].kind == ColumnKind::Unknown {
            let kind = guess_kind(table.rows.iter().map(|row| row.get(index).unwrap_or(&Cell::Null)));
            table.columns[index].kind = kind;
        }
    }
    table
}

/// A count with its thousands set apart, as "1,204,311".
pub fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// What a cell is when it's put in order: nothing, a number, or words.
#[derive(Clone, Debug, PartialEq)]
pub enum Key {
    Null,
    Number(f64),
    Words(String),
}

impl Key {
    /// Order for sorting: numbers (dates among them) by size, words by their lower-case letters, and empty
    /// cells after everything. Numbers come before words in a column that has both.
    fn rank(&self) -> u8 {
        match self {
            Key::Number(_) => 0,
            Key::Words(_) => 1,
            Key::Null => 2,
        }
    }

    fn compare(&self, other: &Key) -> Ordering {
        match (self, other) {
            (Key::Number(a), Key::Number(b)) => a.total_cmp(b),
            (Key::Words(a), Key::Words(b)) => a.cmp(b),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

/// The key to put a cell in order by, given what its column holds.
pub fn key_of(cell: &Cell, kind: ColumnKind) -> Key {
    match cell {
        Cell::Null => Key::Null,
        Cell::Int(n) => Key::Number(*n as f64),
        Cell::Real(x) if x.is_nan() => Key::Null,
        Cell::Real(x) => Key::Number(*x),
        Cell::Bool(b) => Key::Number(f64::from(u8::from(*b))),
        Cell::Bytes(_) => Key::Words(cell.text()),
        Cell::Text(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Key::Null;
            }
            let number = match kind {
                ColumnKind::Integer | ColumnKind::Real => crate::table::number(trimmed),
                ColumnKind::Date => parse_date(trimmed).map(|d| (d * 86_400) as f64),
                ColumnKind::DateTime => parse_date_time(trimmed).or_else(|| parse_date(trimmed).map(|d| (d * 86_400) as f64)),
                ColumnKind::Bool => match trimmed.to_ascii_lowercase().as_str() {
                    "true" => Some(1.0),
                    "false" => Some(0.0),
                    _ => None,
                },
                _ => None,
            };
            number.map_or_else(|| Key::Words(text.to_lowercase()), Key::Number)
        }
    }
}

/// Whether `hay` has `needle_lower` in it, ignoring case.
fn contains_ci(hay: &str, needle_lower: &str) -> bool {
    if hay.is_ascii() && needle_lower.is_ascii() {
        let needle = needle_lower.as_bytes();
        return hay.len() >= needle.len() && hay.as_bytes().windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle));
    }
    hay.to_lowercase().contains(needle_lower)
}

/// Whether any cell in the row has the (lower-case) words in it.
fn row_matches(row: &[Cell], needle_lower: &str, scratch: &mut String) -> bool {
    use std::fmt::Write;
    row.iter().any(|cell| match cell {
        Cell::Null => false,
        Cell::Text(text) => contains_ci(text, needle_lower),
        Cell::Int(n) => {
            scratch.clear();
            let _ = write!(scratch, "{n}");
            scratch.contains(needle_lower)
        }
        other => contains_ci(&other.text(), needle_lower),
    })
}

/// The rows to show, as their places in the table: those with `filter` in some cell, put in the order of the
/// column `order` says (and, if its second part is true, backwards; empty cells always come last). Sorting
/// keeps rows with equal values in their own order. `stop` is looked at now and then, and if it's set, the work
/// is dropped (None).
pub fn view_rows(table: &DataTable, filter: &str, order: Option<(usize, bool)>, stop: &AtomicBool) -> Option<Vec<u32>> {
    let needle = filter.trim().to_lowercase();
    let mut rows: Vec<u32> = if needle.is_empty() {
        (0..table.rows.len() as u32).collect()
    } else {
        let mut scratch = String::new();
        let mut found = Vec::new();
        for (index, row) in table.rows.iter().enumerate() {
            if index.is_multiple_of(4096) && stop.load(AtomicOrdering::Relaxed) {
                return None;
            }
            if row_matches(row, &needle, &mut scratch) {
                found.push(index as u32);
            }
        }
        found
    };
    if let Some((column, backwards)) = order.filter(|(column, _)| *column < table.columns.len()) {
        let kind = table.columns[column].kind;
        let keys: Vec<Key> = table.rows.iter().map(|row| row.get(column).map_or(Key::Null, |cell| key_of(cell, kind))).collect();
        if stop.load(AtomicOrdering::Relaxed) {
            return None;
        }
        rows.sort_by(|&a, &b| {
            let (a, b) = (&keys[a as usize], &keys[b as usize]);
            match (a, b) {
                (Key::Null, _) | (_, Key::Null) => a.compare(b),
                _ if backwards => b.compare(a),
                _ => a.compare(b),
            }
        });
    }
    Some(rows)
}

/// What one column holds, summed up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub count: usize,
    pub empty: usize,
    pub distinct: usize,
    pub smallest: Option<String>,
    pub largest: Option<String>,
    /// For columns of numbers.
    pub mean: Option<f64>,
    /// The values that turn up most, with how many times each, commonest first.
    pub common: Vec<(String, usize)>,
}

/// How many of the commonest values a summary lists.
pub const COMMON_VALUES: usize = 5;

/// Sums up column `column` of the table.
pub fn summarise(table: &DataTable, column: usize, stop: &AtomicBool) -> Option<Summary> {
    let kind = table.columns.get(column)?.kind;
    let mut summary = Summary { count: table.rows.len(), ..Summary::default() };
    let mut seen: HashMap<String, usize> = HashMap::new();
    let (mut smallest, mut largest): (Option<(Key, &Cell)>, Option<(Key, &Cell)>) = (None, None);
    let (mut sum, mut numbers) = (0.0f64, 0usize);
    let numeric = matches!(kind, ColumnKind::Integer | ColumnKind::Real);
    for (index, row) in table.rows.iter().enumerate() {
        if index.is_multiple_of(4096) && stop.load(AtomicOrdering::Relaxed) {
            return None;
        }
        let cell = row.get(column).unwrap_or(&Cell::Null);
        let key = key_of(cell, kind);
        if key == Key::Null {
            summary.empty += 1;
            continue;
        }
        if numeric && let Key::Number(n) = key {
            sum += n;
            numbers += 1;
        }
        let text = cell.text();
        if let Some(count) = seen.get_mut(&text) {
            *count += 1;
        } else {
            seen.insert(text, 1);
        }
        if smallest.as_ref().is_none_or(|(k, _)| key.compare(k) == Ordering::Less) {
            smallest = Some((key.clone(), cell));
        }
        if largest.as_ref().is_none_or(|(k, _)| key.compare(k) == Ordering::Greater) {
            largest = Some((key, cell));
        }
    }
    summary.distinct = seen.len();
    summary.smallest = smallest.map(|(_, cell)| cell.text());
    summary.largest = largest.map(|(_, cell)| cell.text());
    if numbers > 0 {
        summary.mean = Some(sum / numbers as f64);
    }
    let mut common: Vec<(String, usize)> = seen.into_iter().collect();
    common.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    common.truncate(COMMON_VALUES);
    summary.common = common;
    Some(summary)
}

/// The cells from `top_left` to `bottom_right` (as places among the shown rows and the columns), as text.
pub fn block(table: &DataTable, rows: &[u32], top_left: (usize, usize), bottom_right: (usize, usize)) -> Vec<Vec<String>> {
    let last_row = bottom_right.0.min(rows.len().saturating_sub(1));
    (top_left.0..=last_row)
        .map(|r| {
            let row = &table.rows[rows[r] as usize];
            (top_left.1..=bottom_right.1).map(|c| row.get(c).map(Cell::text).unwrap_or_default()).collect()
        })
        .collect()
}

/// The cells as the existing table view copies them: tab-separated, quoted where a cell needs it.
pub fn copy_text(table: &DataTable, rows: &[u32], top_left: (usize, usize), bottom_right: (usize, usize)) -> String {
    if rows.is_empty() {
        return String::new();
    }
    crate::table::block_text(&block(table, rows, top_left, bottom_right))
}

/// The rows given, as CSV with a heading row: commas between cells, quotes where a cell needs them.
pub fn to_csv(table: &DataTable, rows: &[u32]) -> String {
    let line = |cells: Vec<String>| cells.iter().map(|v| crate::table::encode_field(v, ',', false)).collect::<Vec<_>>().join(",");
    let mut out = line(table.columns.iter().map(|c| c.name.clone()).collect());
    out.push('\n');
    for &row in rows {
        out.push_str(&line(table.rows[row as usize].iter().map(Cell::text).collect()));
        out.push('\n');
    }
    out
}

/// Something worked out on another thread, which the window collects when it's done and can drop to stop.
pub struct Job<T> {
    done: std::sync::mpsc::Receiver<T>,
    stop: Arc<AtomicBool>,
}

impl<T: Send + 'static> Job<T> {
    pub fn start(ctx: Option<eframe::egui::Context>, work: impl FnOnce(&AtomicBool) -> Option<T> + Send + 'static) -> Self {
        let (send, done) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            if let Some(result) = work(&flag) {
                let _ = send.send(result);
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            }
        });
        Job { done, stop }
    }

    /// The result, if it's ready.
    pub fn take(&self) -> Option<T> {
        self.done.try_recv().ok()
    }
}

impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.stop.store(true, AtomicOrdering::Relaxed);
    }
}

/// What the rows shown depend on.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewKey {
    pub filter: String,
    pub order: Option<(usize, bool)>,
}

/// One sheet in the viewer: its table and how it's being looked at.
pub struct SheetState {
    pub name: String,
    pub table: Arc<DataTable>,
    pub filter: String,
    /// The column sorted by, and whether backwards.
    pub order: Option<(usize, bool)>,
    /// The rows shown, as places in the table.
    pub rows: Arc<Vec<u32>>,
    pub rows_for: ViewKey,
    pub working: Option<(ViewKey, Job<Vec<u32>>)>,
    /// A cell chosen, as a place among the rows shown and a column; and where a dragged block began.
    pub cursor: (usize, usize),
    pub anchor: Option<(usize, usize)>,
    pub widths: Vec<f32>,
    pub summaries: HashMap<usize, Summary>,
    pub summarising: Option<(usize, Job<Summary>)>,
    pub scroll_to_cursor: bool,
}

impl SheetState {
    pub fn new(name: String, table: DataTable) -> Self {
        let table = with_kinds_guessed(table);
        let rows: Vec<u32> = (0..table.rows.len() as u32).collect();
        SheetState {
            name,
            table: Arc::new(table),
            filter: String::new(),
            order: None,
            rows: Arc::new(rows),
            rows_for: ViewKey { filter: String::new(), order: None },
            working: None,
            cursor: (0, 0),
            anchor: None,
            widths: Vec::new(),
            summaries: HashMap::new(),
            summarising: None,
            scroll_to_cursor: false,
        }
    }

    fn wanted(&self) -> ViewKey {
        ViewKey { filter: self.filter.trim().to_string(), order: self.order }
    }

    /// Whether rows are still being found or put in order.
    pub fn busy(&self) -> bool {
        self.working.is_some() || self.summarising.is_some()
    }

    /// Starts on the rows the filter and order ask for if they've changed, and takes them in when they're ready.
    pub fn update(&mut self, ctx: Option<&eframe::egui::Context>) {
        if let Some((key, job)) = &self.working
            && let Some(rows) = job.take()
        {
            self.rows_for = key.clone();
            self.rows = Arc::new(rows);
            self.working = None;
            self.cursor.0 = self.cursor.0.min(self.rows.len().saturating_sub(1));
            if self.anchor.is_some_and(|a| a.0 >= self.rows.len()) {
                self.anchor = None;
            }
        }
        let wanted = self.wanted();
        if wanted != self.rows_for && self.working.as_ref().is_none_or(|(key, _)| *key != wanted) {
            let (table, key) = (self.table.clone(), wanted.clone());
            let (filter, order) = (key.filter.clone(), key.order);
            self.working = Some((wanted, Job::start(ctx.cloned(), move |stop| view_rows(&table, &filter, order, stop))));
        }
        if let Some((column, job)) = &self.summarising
            && let Some(summary) = job.take()
        {
            self.summaries.insert(*column, summary);
            self.summarising = None;
        }
    }

    /// The summary of a column, started if it hasn't been worked out.
    pub fn summary(&mut self, column: usize, ctx: Option<&eframe::egui::Context>) -> Option<&Summary> {
        if !self.summaries.contains_key(&column) && self.summarising.as_ref().is_none_or(|(c, _)| *c != column) && column < self.table.columns.len() {
            let table = self.table.clone();
            self.summarising = Some((column, Job::start(ctx.cloned(), move |stop| summarise(&table, column, stop))));
        }
        self.summaries.get(&column)
    }

    /// A click on a column's heading: sorts by it, then backwards, then goes back to the table's own order.
    pub fn click_heading(&mut self, column: usize) {
        self.order = match self.order {
            Some((c, false)) if c == column => Some((column, true)),
            Some((c, true)) if c == column => None,
            _ => Some((column, false)),
        };
    }

    /// The block of cells chosen, corners in order.
    pub fn selection(&self) -> ((usize, usize), (usize, usize)) {
        let (a, b) = (self.anchor.unwrap_or(self.cursor), self.cursor);
        ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))
    }

    pub fn copy_text(&self) -> String {
        let (top_left, bottom_right) = self.selection();
        copy_text(&self.table, &self.rows, top_left, bottom_right)
    }
}

/// What the viewer holds: the sheets of a workbook, or the one table, and which is in front.
pub struct DataState {
    pub sheets: Vec<SheetState>,
    pub active: usize,
    /// Whether the strip at the side with the column's summary is shown.
    pub strip: bool,
    /// The text file it was read from, if it can also be opened as text (JSON lines).
    pub from: Option<std::path::PathBuf>,
}

impl DataState {
    pub fn new(sheets: Vec<(String, DataTable)>) -> Self {
        DataState { sheets: sheets.into_iter().map(|(name, table)| SheetState::new(name, table)).collect(), active: 0, strip: true, from: None }
    }

    pub fn busy(&self) -> bool {
        self.sheets.iter().any(SheetState::busy)
    }

    pub fn sheet(&mut self) -> Option<&mut SheetState> {
        self.sheets.get_mut(self.active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_table::DataColumn;

    fn text(s: &str) -> Cell {
        Cell::Text(s.into())
    }

    fn table(columns: &[(&str, ColumnKind)], rows: Vec<Vec<Cell>>) -> DataTable {
        DataTable { columns: columns.iter().map(|(name, kind)| DataColumn { name: (*name).into(), kind: *kind }).collect(), rows, note: None }
    }

    fn rows_of(table: &DataTable, filter: &str, order: Option<(usize, bool)>) -> Vec<u32> {
        view_rows(table, filter, order, &AtomicBool::new(false)).unwrap()
    }

    #[test]
    fn counts_have_their_thousands_set_apart() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(1_204_311), "1,204,311");
    }

    #[test]
    fn calendar_arithmetic_round_trips() {
        assert_eq!(days_from_civil(1970, 1, 1), Some(0));
        assert_eq!(days_from_civil(2000, 2, 29), Some(11_016));
        assert_eq!(days_from_civil(1900, 2, 29), None);
        assert_eq!(days_from_civil(2026, 2, 29), None);
        assert_eq!(days_from_civil(2026, 13, 1), None);
        for days in [-25_567, -1, 0, 1, 11_016, 20_000, 47_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), Some(days));
        }
    }

    #[test]
    fn dates_and_times_are_read_in_the_usual_ways() {
        assert_eq!(parse_date("2026-11-22"), days_from_civil(2026, 11, 22));
        assert_eq!(parse_date("2026/1/05"), days_from_civil(2026, 1, 5));
        assert_eq!(parse_date("2026-01-5"), None, "the day needs two digits");
        assert_eq!(parse_date("22-11-2026"), None);
        assert_eq!(parse_date("2026-02-30"), None);
        assert_eq!(parse_date_time("1970-01-01 00:00:10"), Some(10.0));
        assert_eq!(parse_date_time("1970-01-01T01:00:00Z"), Some(3600.0));
        assert_eq!(parse_date_time("1970-01-01T01:00:00+01:00"), Some(0.0));
        assert_eq!(parse_date_time("1970-01-01T00:00:00.5"), Some(0.5));
        assert_eq!(parse_date_time("1970-01-01 25:00:00"), None);
        assert_eq!(parse_date_time("hello world"), None);
    }

    #[test]
    fn kinds_are_guessed_from_the_values() {
        let kind = |values: &[&str]| {
            let cells: Vec<Cell> = values.iter().map(|v| if v.is_empty() { Cell::Null } else { text(v) }).collect();
            guess_kind(cells.iter())
        };
        assert_eq!(kind(&["1", "-20", "", "300"]), ColumnKind::Integer);
        assert_eq!(kind(&["1", "2.5", "1e3"]), ColumnKind::Real);
        assert_eq!(kind(&["true", "False"]), ColumnKind::Bool);
        assert_eq!(kind(&["2026-01-02", "1999-12-31"]), ColumnKind::Date);
        assert_eq!(kind(&["2026-01-02", "1999-12-31 10:00"]), ColumnKind::DateTime);
        assert_eq!(kind(&["2026-01-02 10:00:00", "2026-01-02T11:00:00Z"]), ColumnKind::DateTime);
        assert_eq!(kind(&["{\"a\":1}", "[1]"]), ColumnKind::Json);
        assert_eq!(kind(&["1", "two"]), ColumnKind::Text);
        assert_eq!(kind(&["007", "012"]), ColumnKind::Text, "leading zeros are an identifier, not a number");
        assert_eq!(kind(&["nan", "inf"]), ColumnKind::Text);
        assert_eq!(kind(&["", ""]), ColumnKind::Text);
        assert_eq!(guess_kind([Cell::Int(1), Cell::Real(2.5), Cell::Null].iter()), ColumnKind::Real);
        assert_eq!(guess_kind([Cell::Bool(true)].iter()), ColumnKind::Bool);
        let t = with_kinds_guessed(table(&[("a", ColumnKind::Unknown), ("b", ColumnKind::Text)], vec![vec![text("1"), text("2")]]));
        assert_eq!((t.columns[0].kind, t.columns[1].kind), (ColumnKind::Integer, ColumnKind::Text), "a kind that's said isn't guessed over");
    }

    #[test]
    fn numbers_sort_as_numbers_and_dates_as_dates() {
        let t = with_kinds_guessed(table(
            &[("n", ColumnKind::Unknown), ("d", ColumnKind::Unknown), ("w", ColumnKind::Unknown)],
            vec![
                vec![text("10"), text("2026-03-01"), text("banana")],
                vec![text("9"), text("2025-12-31"), text("Apple")],
                vec![Cell::Null, text("2026-01-15"), text("")],
                vec![text("100"), Cell::Null, text("cherry")],
            ],
        ));
        assert_eq!(rows_of(&t, "", Some((0, false))), [1, 0, 3, 2], "9, 10, 100, then the empty one last");
        assert_eq!(rows_of(&t, "", Some((0, true))), [3, 0, 1, 2], "backwards, with the empty one still last");
        assert_eq!(rows_of(&t, "", Some((1, false))), [1, 2, 0, 3]);
        assert_eq!(rows_of(&t, "", Some((2, false))), [1, 0, 3, 2], "words ignore case");
        assert_eq!(rows_of(&t, "", None), [0, 1, 2, 3]);
    }

    #[test]
    fn equal_values_keep_their_order_and_a_bad_column_is_ignored() {
        let t = table(&[("k", ColumnKind::Integer)], vec![vec![Cell::Int(1)], vec![Cell::Int(0)], vec![Cell::Int(1)], vec![Cell::Int(0)]]);
        assert_eq!(rows_of(&t, "", Some((0, false))), [1, 3, 0, 2]);
        assert_eq!(rows_of(&t, "", Some((0, true))), [0, 2, 1, 3]);
        assert_eq!(rows_of(&t, "", Some((9, false))), [0, 1, 2, 3]);
    }

    #[test]
    fn typing_filters_the_rows_whatever_the_case_or_the_kind_of_cell() {
        let t = table(
            &[("a", ColumnKind::Text), ("b", ColumnKind::Integer)],
            vec![vec![text("Ada Lovelace"), Cell::Int(36)], vec![text("grace HOPPER"), Cell::Int(85)], vec![text("Éloïse"), Cell::Null], vec![Cell::Null, Cell::Int(3600)]],
        );
        assert_eq!(rows_of(&t, "ada", None), [0]);
        assert_eq!(rows_of(&t, "HOPPER", None), [1]);
        assert_eq!(rows_of(&t, "36", None), [0, 3], "numbers match as text");
        assert_eq!(rows_of(&t, "éLO", None), [2], "also with accents");
        assert_eq!(rows_of(&t, "nothing", None), Vec::<u32>::new());
        assert_eq!(rows_of(&t, "  ", None), [0, 1, 2, 3], "spaces alone filter nothing");
        assert_eq!(rows_of(&t, "a", Some((1, true))), [1, 0], "filtered, then in order");
    }

    #[test]
    fn work_that_is_stopped_gives_nothing() {
        let t = table(&[("a", ColumnKind::Text)], vec![vec![text("x")]; 10_000]);
        assert!(view_rows(&t, "x", None, &AtomicBool::new(true)).is_none());
        assert!(summarise(&t, 0, &AtomicBool::new(true)).is_none());
    }

    #[test]
    fn a_column_is_summed_up() {
        let t = table(&[("n", ColumnKind::Integer), ("w", ColumnKind::Text)], vec![vec![Cell::Int(4), text("x")], vec![Cell::Int(2), text("y")], vec![Cell::Null, text("x")], vec![Cell::Int(4), text("")], vec![Cell::Int(10), text("z")]]);
        let numbers = summarise(&t, 0, &AtomicBool::new(false)).unwrap();
        assert_eq!((numbers.count, numbers.empty, numbers.distinct), (5, 1, 3));
        assert_eq!((numbers.smallest.as_deref(), numbers.largest.as_deref()), (Some("2"), Some("10")));
        assert_eq!(numbers.mean, Some(5.0));
        assert_eq!(numbers.common[0], ("4".to_string(), 2));
        let words = summarise(&t, 1, &AtomicBool::new(false)).unwrap();
        assert_eq!((words.count, words.empty, words.distinct), (5, 1, 3), "a blank text counts as empty");
        assert_eq!(words.mean, None);
        assert_eq!(words.common, [("x".to_string(), 2), ("y".to_string(), 1), ("z".to_string(), 1)], "ties by name");
        assert_eq!((words.smallest.as_deref(), words.largest.as_deref()), (Some("x"), Some("z")));
        assert!(summarise(&t, 7, &AtomicBool::new(false)).is_none());
    }

    #[test]
    fn dates_have_their_smallest_and_largest_as_dates() {
        let t = with_kinds_guessed(table(&[("d", ColumnKind::Unknown)], vec![vec![text("2026-03-01")], vec![text("1999-12-31")], vec![text("2025-01-01")]]));
        let s = summarise(&t, 0, &AtomicBool::new(false)).unwrap();
        assert_eq!((s.smallest.as_deref(), s.largest.as_deref()), (Some("1999-12-31"), Some("2026-03-01")));
        assert_eq!(s.mean, None);
    }

    #[test]
    fn cells_are_copied_as_the_table_view_copies_them() {
        let t = table(&[("a", ColumnKind::Text), ("b", ColumnKind::Text)], vec![vec![text("x"), text("has\ttab")], vec![text("q\"uote"), Cell::Int(5)], vec![Cell::Null, Cell::Bytes(vec![1, 255])]]);
        let rows = [2u32, 1, 0];
        assert_eq!(copy_text(&t, &rows, (0, 0), (1, 1)), "\t01FF\n\"q\"\"uote\"\t5");
        assert_eq!(copy_text(&t, &rows, (2, 1), (9, 1)), "\"has\ttab\"", "a block past the end stops at the last row");
        assert_eq!(copy_text(&t, &[], (0, 0), (0, 0)), "");
    }

    #[test]
    fn csv_has_a_heading_row_and_quotes_what_needs_them() {
        let t = table(&[("name", ColumnKind::Text), ("note, long", ColumnKind::Text)], vec![vec![text("a"), text("x,y")], vec![text("b"), text("say \"hi\"\nnow")], vec![Cell::Null, Cell::Real(2.5)]]);
        assert_eq!(to_csv(&t, &[0, 1, 2]), "name,\"note, long\"\na,\"x,y\"\nb,\"say \"\"hi\"\"\nnow\"\n,2.5\n");
        assert_eq!(to_csv(&t, &[2]), "name,\"note, long\"\n,2.5\n", "only the rows given, in that order");
    }

    #[test]
    fn a_heading_click_goes_round_ascending_descending_and_off() {
        let mut sheet = SheetState::new("s".into(), table(&[("a", ColumnKind::Integer), ("b", ColumnKind::Integer)], vec![]));
        sheet.click_heading(1);
        assert_eq!(sheet.order, Some((1, false)));
        sheet.click_heading(1);
        assert_eq!(sheet.order, Some((1, true)));
        sheet.click_heading(0);
        assert_eq!(sheet.order, Some((0, false)), "another column starts again");
        sheet.click_heading(0);
        sheet.click_heading(0);
        assert_eq!(sheet.order, None);
    }

    #[test]
    fn a_sheet_follows_what_is_typed_and_what_is_clicked() {
        let mut sheet = SheetState::new("s".into(), table(&[("a", ColumnKind::Unknown)], vec![vec![text("3")], vec![text("1")], vec![text("2")]]));
        assert_eq!(sheet.table.columns[0].kind, ColumnKind::Integer);
        sheet.filter = "1".into();
        let wait = |sheet: &mut SheetState| {
            for _ in 0..500 {
                sheet.update(None);
                if !sheet.busy() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            panic!("the rows weren't found");
        };
        wait(&mut sheet);
        assert_eq!(*sheet.rows, [1]);
        sheet.filter.clear();
        sheet.click_heading(0);
        wait(&mut sheet);
        assert_eq!(*sheet.rows, [1, 2, 0]);
        sheet.cursor = (1, 0);
        sheet.anchor = Some((2, 0));
        assert_eq!(sheet.selection(), ((1, 0), (2, 0)));
        assert_eq!(sheet.copy_text(), "2\n3");
        assert_eq!(sheet.summary(0, None), None, "started, not ready");
        for _ in 0..500 {
            sheet.update(None);
            if sheet.summarising.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(sheet.summary(0, None).map(|s| s.distinct), Some(3));
    }

    /// A million rows is sorted and filtered at once, and quickly.
    #[test]
    fn a_million_rows_are_sorted_filtered_and_summed_up() {
        let rows: Vec<Vec<Cell>> = (0..1_000_000u64).map(|i| vec![Cell::Int(((i * 7919) % 1_000_003) as i64), Cell::Text(format!("row {}", i % 1000))]).collect();
        let t = with_kinds_guessed(DataTable { columns: vec![DataColumn { name: "n".into(), kind: ColumnKind::Unknown }, DataColumn { name: "w".into(), kind: ColumnKind::Unknown }], rows, note: None });
        assert_eq!(t.columns[0].kind, ColumnKind::Integer);
        let started = std::time::Instant::now();
        let sorted = rows_of(&t, "", Some((0, false)));
        assert_eq!(sorted.len(), 1_000_000);
        assert!(sorted.windows(2).all(|w| key_of(&t.rows[w[0] as usize][0], ColumnKind::Integer).compare(&key_of(&t.rows[w[1] as usize][0], ColumnKind::Integer)) != Ordering::Greater));
        let filtered = rows_of(&t, "row 999", None);
        assert_eq!(filtered.len(), 1000);
        let summary = summarise(&t, 1, &AtomicBool::new(false)).unwrap();
        assert_eq!(summary.distinct, 1000);
        assert!(started.elapsed().as_secs() < 30, "took {:?}", started.elapsed());
    }
}
