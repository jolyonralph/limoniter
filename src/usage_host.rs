//! The Usage window (Tools, Usage; also a button at the top of the AI agents panel): what Claude Code and Codex
//! have used today or this week, added up from the logs they keep (so sessions run in a terminal count), the plan
//! limits from the newest source there is, and the sessions running now. The logs are read on a thread of their own,
//! only what has been added since last time. This file also applies the Settings switch that lets Claude's status
//! line tell Limoniter its limits for terminal sessions.

use super::*;
use crate::agent_panel::{compact, reset_text};
use crate::agent_session::{Limits, Tool};
use crate::usage_logs::{self, Counts, Fresh, Range, Running, Scanner, Seen, Source, Which};
use crate::usage_status::{self, Record};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

type Total = (Which, String, Counts);

/// Everything the window shows that comes from the logs.
#[derive(Clone, Default)]
pub(super) struct Snapshot {
    pub at: i64,
    pub today: Vec<Total>,
    pub week: Vec<Total>,
    pub running: Vec<Running>,
    pub codex_limits: Option<(i64, Limits)>,
    pub records: Vec<Record>,
}

/// The thread that reads the logs, which stops when this is let go.
struct Worker {
    alive: Arc<AtomicBool>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}

pub(super) struct State {
    pub open: bool,
    range: Range,
    worker: Option<Worker>,
    shared: Arc<Mutex<Snapshot>>,
    /// Made-up figures for a screenshot, shown in place of the logs'.
    fake: Option<Snapshot>,
    /// What each panel session last said of the limits, and when it changed.
    panel_seen: HashMap<u64, (Limits, i64)>,
    /// What the Settings switch for Claude's status line was last made to be.
    applied: Option<bool>,
    applying: Option<Receiver<Vec<String>>>,
}

impl Default for State {
    fn default() -> Self {
        State { open: false, range: Range::Today, worker: None, shared: Arc::new(Mutex::new(Snapshot::default())), fake: None, panel_seen: HashMap::new(), applied: None, applying: None }
    }
}

fn now_seconds() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Reads the logs again and again, until told to stop.
fn work(shared: Arc<Mutex<Snapshot>>, alive: Arc<AtomicBool>, wake: Context) {
    let mut scanner = Scanner::default();
    while alive.load(Ordering::Relaxed) {
        let now = now_seconds();
        let roots = usage_logs::roots();
        scanner.scan(&roots, now);
        let records = roots.iter().filter(|r| r.which == Which::Claude).flat_map(|r| usage_status::read_records(&r.dir)).collect();
        let snapshot = Snapshot { at: now, today: scanner.totals(Range::Today, now), week: scanner.totals(Range::Week, now), running: scanner.running.clone(), codex_limits: scanner.codex_limits, records };
        if let Ok(mut shared) = shared.lock() {
            *shared = snapshot;
        }
        wake.request_repaint();
        for _ in 0..8 {
            if !alive.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
}

/// Where the status line command goes, for the Claude Code settings folders there are.
fn status_targets() -> Vec<usage_status::Target> {
    let Ok(exe) = std::env::current_exe() else { return Vec::new() };
    usage_logs::roots()
        .into_iter()
        .filter(|root| root.which == Which::Claude && root.dir.is_dir())
        .map(|root| if root.dir.display().to_string().starts_with("\\\\") { usage_status::Target::in_wsl(&root.dir, &exe) } else { usage_status::Target::here(&root.dir, &exe) })
        .collect()
}

/// Puts the status line command in, or takes it out, everywhere there is; what went wrong is returned.
fn apply_status_line(on: bool, say_if_none: bool) -> Vec<String> {
    let targets = status_targets();
    if targets.is_empty() {
        return if on && say_if_none { vec![crate::i18n::tr("Claude Code's settings folder wasn't found, so there was nowhere to put the status line.").to_string()] } else { Vec::new() };
    }
    let mut problems = Vec::new();
    for target in targets {
        let result = if on { usage_status::install(&target) } else { usage_status::remove(&target.config) };
        if let Err(why) = result {
            problems.push(why);
        }
    }
    problems
}

/// Words for how long ago, for the age of a figure.
fn ago(seconds: i64) -> String {
    match seconds.max(0) {
        s if s < 60 => crate::i18n::tr("just now").to_string(),
        s if s < 3600 => crate::i18n::tf!("{} min ago", s / 60),
        s if s < 86400 => crate::i18n::tf!("{} h ago", s / 3600),
        s => crate::i18n::tf!("{} days ago", s / 86400),
    }
}

fn source_name(from: Source) -> String {
    match from {
        Source::Panel => crate::i18n::tr("an AI agents session"),
        Source::StatusLine => crate::i18n::tr("Claude's status line"),
        Source::Log => crate::i18n::tr("its log"),
    }
    .to_string()
}

fn one_limit(ui: &mut Ui, name: &str, fresh: Option<Fresh>, now: i64) {
    ui.label(egui::RichText::new(name).color(theme::text_dim()));
    let Some(fresh) = fresh else {
        ui.label(egui::RichText::new(crate::i18n::tr("not known")).color(theme::text_faint()));
        ui.end_row();
        return;
    };
    ui.horizontal(|ui| {
        if fresh.has_reset(now) {
            ui.label(egui::RichText::new(crate::i18n::tf!("was {}%, has since reset", fresh.limit.percent.round() as i64)).color(theme::text_faint()));
        } else {
            let bar = egui::ProgressBar::new((fresh.limit.percent / 100.0).clamp(0.0, 1.0)).desired_width(110.0).text(format!("{}%", fresh.limit.percent.round() as i64));
            ui.add(bar);
            if let Some(resets) = fresh.limit.resets {
                ui.label(egui::RichText::new(crate::i18n::tf!("resets {}", reset_text(resets, now))).color(theme::text_dim()));
            }
        }
        ui.label(egui::RichText::new(crate::i18n::tf!("from {}, {}", source_name(fresh.from), ago(now - fresh.at))).color(theme::text_faint()).size(11.0));
    });
    ui.end_row();
}

fn folder_name(folder: &str) -> String {
    folder.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(folder).to_string()
}

impl Limoniter {
    /// Opens the window, and starts reading the logs.
    pub(super) fn open_usage(&mut self, ctx: &Context) {
        self.usage.open = true;
        self.start_usage_worker(ctx);
    }

    fn start_usage_worker(&mut self, ctx: &Context) {
        if self.usage.worker.is_none() && self.usage.fake.is_none() {
            let alive = Arc::new(AtomicBool::new(true));
            let (shared, flag, wake) = (self.usage.shared.clone(), alive.clone(), ctx.clone());
            std::thread::spawn(move || work(shared, flag, wake));
            self.usage.worker = Some(Worker { alive });
        }
    }

    /// Notes what the panel's sessions say of the limits, and makes Claude's status line as Settings says.
    pub(super) fn usage_tick(&mut self, _ctx: &Context) {
        let now = now_seconds();
        let mut numbers = Vec::new();
        for tab in &self.agents.tabs {
            numbers.push(tab.number);
            let limits = tab.session.limits;
            if limits.any() && self.usage.panel_seen.get(&tab.number).is_none_or(|(seen, _)| *seen != limits) {
                self.usage.panel_seen.insert(tab.number, (limits, now));
            }
        }
        self.usage.panel_seen.retain(|number, _| numbers.contains(number));
        if let Some(done) = &self.usage.applying {
            if let Ok(problems) = done.try_recv() {
                self.usage.applying = None;
                if !problems.is_empty() {
                    self.message("Usage", problems.join("\n"));
                }
            }
        } else if self.screenshot.is_none() && self.usage.applied != Some(self.prefs.claude_limits) {
            // At the start too, so that a copy of Limoniter that has moved is the one the status line runs.
            let (on, asked) = (self.prefs.claude_limits, self.usage.applied.is_some());
            self.usage.applied = Some(on);
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(apply_status_line(on, asked));
            });
            self.usage.applying = Some(rx);
        }
        if !self.usage.open {
            self.usage.worker = None;
        }
    }

    /// What each place says of the limits, as the figures to choose the newest from.
    fn usage_seen(&self, snapshot: &Snapshot, tool: Tool) -> Vec<Seen> {
        let mut seen = Vec::new();
        for tab in self.agents.tabs.iter().filter(|t| t.session.tool == tool) {
            if let Some((limits, at)) = self.usage.panel_seen.get(&tab.number) {
                seen.push(Seen { limits: *limits, at: *at, from: Source::Panel });
            }
        }
        match tool {
            Tool::Claude => seen.extend(snapshot.records.iter().filter(|r| r.limits.any()).map(|r| Seen { limits: r.limits, at: r.at, from: Source::StatusLine })),
            Tool::Codex => seen.extend(snapshot.codex_limits.map(|(at, limits)| Seen { limits, at, from: Source::Log })),
        }
        seen
    }

    pub(super) fn usage_window(&mut self, ctx: &Context) {
        if !self.usage.open {
            return;
        }
        self.start_usage_worker(ctx);
        let snapshot = self.usage.fake.clone().unwrap_or_else(|| self.usage.shared.lock().map(|s| s.clone()).unwrap_or_default());
        let now = if self.usage.fake.is_some() { snapshot.at } else { now_seconds() };
        let mut open = true;
        let mut range = self.usage.range;
        let mut turn_on = false;
        let limits_on = self.prefs.claude_limits;
        let ready = snapshot.at > 0;
        let seen = (self.usage_seen(&snapshot, Tool::Claude), self.usage_seen(&snapshot, Tool::Codex));
        let panel: Vec<(String, Option<f64>, Option<(u64, u64)>)> = self.agents.tabs.iter().filter(|t| t.session.finished.is_none()).filter_map(|t| t.session.session_id.clone().map(|id| (id, t.session.cost, t.session.context.map(|c| (c.used, c.max))))).collect();
        crate::fit::Dialog::new(crate::i18n::tr("Usage")).open(&mut open).collapsible(false).resizable(true).default_width(560.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_min_width(520.0);
            // The plan's limits first: they say how much is left.
            for (which, tool, seen) in [(Which::Claude, Tool::Claude, &seen.0), (Which::Codex, Tool::Codex, &seen.1)] {
                let (five, week) = usage_logs::freshest(seen, now);
                ui.label(egui::RichText::new(which.name()).strong());
                egui::Grid::new(("usage-limits", which as u8)).num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                    one_limit(ui, crate::i18n::tr("Five hours"), five, now);
                    one_limit(ui, crate::i18n::tr("Week"), week, now);
                });
                if tool == Tool::Claude && five.is_none() && week.is_none() {
                    ui.label(egui::RichText::new(crate::i18n::tr("Claude only says in a session of its own, here or in a terminal with the switch below on.")).color(theme::text_faint()).size(11.5));
                }
                ui.add_space(4.0);
            }
            if !limits_on {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(crate::i18n::tr("Claude's limits for sessions in a terminal need a status line command in its settings.")).color(theme::text_dim()).size(11.5));
                    if ui.small_button(crate::i18n::tr("Turn this on")).clicked() {
                        turn_on = true;
                    }
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.radio_value(&mut range, Range::Today, crate::i18n::tr("Today"));
                ui.radio_value(&mut range, Range::Week, crate::i18n::tr("This week"));
                ui.label(egui::RichText::new(crate::i18n::tr("from the tools' own logs, so sessions in a terminal count too")).color(theme::text_faint()).size(11.0));
            });
            let totals = if range == Range::Today { &snapshot.today } else { &snapshot.week };
            if !ready {
                ui.label(egui::RichText::new(crate::i18n::tr("Reading the logs\u{2026}")).color(theme::text_faint()));
            } else if totals.is_empty() {
                ui.label(egui::RichText::new(crate::i18n::tr("Nothing used in this time.")).color(theme::text_faint()));
            }
            for which in [Which::Claude, Which::Codex] {
                let rows: Vec<&Total> = totals.iter().filter(|t| t.0 == which).collect();
                if rows.is_empty() {
                    continue;
                }
                ui.add_space(4.0);
                ui.label(egui::RichText::new(which.name()).strong());
                egui::Grid::new(("usage-tokens", which as u8)).num_columns(5).spacing([14.0, 3.0]).striped(true).show(ui, |ui| {
                    for head in [crate::i18n::tr("Model"), crate::i18n::tr("In"), crate::i18n::tr("Out"), crate::i18n::tr("Cache read"), crate::i18n::tr("Cache written")] {
                        ui.label(egui::RichText::new(head).color(theme::text_dim()).size(11.5));
                    }
                    ui.end_row();
                    let mut sum = Counts::default();
                    for (_, model, counts) in &rows {
                        ui.label(model.as_str());
                        for n in [counts.input, counts.output, counts.cache_read, counts.cache_write] {
                            ui.label(compact(n));
                        }
                        ui.end_row();
                        sum.add(*counts);
                    }
                    if rows.len() > 1 {
                        ui.label(egui::RichText::new(crate::i18n::tr("All")).strong());
                        for n in [sum.input, sum.output, sum.cache_read, sum.cache_write] {
                            ui.label(egui::RichText::new(compact(n)).strong());
                        }
                        ui.end_row();
                    }
                });
            }
            ui.separator();
            ui.label(egui::RichText::new(crate::i18n::tr("Running now")).strong());
            if snapshot.running.is_empty() {
                ui.label(egui::RichText::new(crate::i18n::tr("No session has written to its log in the last five minutes.")).color(theme::text_faint()));
            }
            for run in &snapshot.running {
                let record = snapshot.records.iter().find(|r| r.session == run.id);
                let panelled = panel.iter().find(|p| p.0 == run.id);
                let cost = panelled.and_then(|p| p.1).or_else(|| record.and_then(|r| r.cost));
                let context = panelled.and_then(|p| p.2).or_else(|| record.and_then(|r| r.context_used.zip(r.context_size))).or_else(|| run.context.and_then(|(used, max)| max.map(|m| (used, m))));
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(run.which.name()).color(theme::text_dim()));
                    ui.label(run.folder.as_deref().map(folder_name).unwrap_or_else(|| run.id.chars().take(8).collect()));
                    if let Some(model) = run.model.as_deref().or(record.and_then(|r| r.model.as_deref())) {
                        ui.label(egui::RichText::new(model).color(theme::text_faint()).size(11.5));
                    }
                    ui.label(egui::RichText::new(crate::i18n::tf!("{} tokens", compact(run.counts.total()))).color(theme::text_dim()).size(11.5));
                    if let Some(cost) = cost {
                        ui.label(egui::RichText::new(format!("${cost:.2}")).color(theme::text_dim()).size(11.5));
                    }
                    if let Some((used, max)) = context.filter(|c| c.1 > 0) {
                        let percent = (used as f32 / max as f32).clamp(0.0, 1.0);
                        ui.add(egui::ProgressBar::new(percent).desired_width(80.0).text(egui::RichText::new(crate::i18n::tf!("Context {}%", (percent * 100.0).round() as i64)).size(10.5))).on_hover_text(crate::i18n::tf!("{} of {} tokens of context used", compact(used), compact(max)));
                    } else if let Some((used, _)) = run.context {
                        ui.label(egui::RichText::new(crate::i18n::tf!("last prompt {} tokens", compact(used))).color(theme::text_faint()).size(11.5));
                    }
                });
            }
        });
        self.usage.range = range;
        if turn_on {
            self.prefs.claude_limits = true;
        }
        if !open {
            self.usage.open = false;
            self.usage.worker = None;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs(2));
        }
    }

    /// The Usage window with made-up figures, for a screenshot.
    pub(super) fn usage_screenshot_step(&mut self, _value: &str) {
        let now = now_seconds();
        let counts = |input, output, cache_read, cache_write| Counts { input, output, cache_read, cache_write };
        let today = vec![
            (Which::Claude, "claude-opus-5-5".to_string(), counts(41_200, 88_400, 6_420_000, 310_000)),
            (Which::Claude, "claude-haiku-4-5".to_string(), counts(3_100, 5_600, 120_000, 22_000)),
            (Which::Codex, "gpt-5.5".to_string(), counts(212_000, 18_900, 1_830_000, 0)),
        ];
        let week = vec![
            (Which::Claude, "claude-opus-5-5".to_string(), counts(310_000, 640_000, 51_000_000, 2_400_000)),
            (Which::Claude, "claude-haiku-4-5".to_string(), counts(20_000, 41_000, 900_000, 150_000)),
            (Which::Codex, "gpt-5.5".to_string(), counts(1_200_000, 120_000, 11_000_000, 0)),
        ];
        let limit = |percent, resets| Some(crate::agent_session::Limit { percent, resets: Some(now + resets) });
        let record = Record { session: "abc".into(), at: now - 40, model: Some("Opus 5.5".into()), folder: None, cost: Some(1.84), context_percent: Some(31.0), context_size: Some(1_000_000), context_used: Some(310_000), tokens_in: None, tokens_out: None, limits: Limits { five_hour: limit(42.0, 3 * 3600), week: limit(19.0, 4 * 86400) } };
        let running = vec![
            Running { which: Which::Claude, id: "abc".into(), folder: Some("/home/ann/work/limoniter".into()), model: Some("claude-opus-5-5".into()), counts: counts(41_200, 88_400, 6_420_000, 310_000), context: None, written: now - 20, path: PathBuf::new() },
            Running { which: Which::Codex, id: "def".into(), folder: Some("/home/ann/work/parser".into()), model: Some("gpt-5.5".into()), counts: counts(212_000, 18_900, 1_830_000, 0), context: Some((96_000, Some(258_400))), written: now - 90, path: PathBuf::new() },
        ];
        self.usage.fake = Some(Snapshot { at: now, today, week, running, codex_limits: Some((now - 300, Limits { five_hour: limit(12.0, 4 * 3600), week: limit(65.0, 2 * 86400) })), records: vec![record] });
        self.usage.open = true;
    }
}
