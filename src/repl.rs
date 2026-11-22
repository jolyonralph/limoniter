//! A Python or R console beside the editor, running on a Jupyter kernel as a notebook does: each piece of code
//! sent to it (typed, or sent from a script with Ctrl+Enter, or a `# %%` cell) becomes an entry in a notebook of
//! its own that's never shown as one, so outputs, errors, `input()` and pictures come as they do in a notebook.
//! The plots it draws are kept, the last few, to go back to; what's typed is kept, to step back through with the
//! arrow keys. Nothing here draws: `python_host` shows it.

use crate::jupyter::link::{KernelSpec, KernelStatus};
use crate::notebook::{CellKind, Notebook};
use crate::notebook_output::{self as output, Shown};
use crate::notebook_run::{CellState, Kernels, Session};
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

/// How many plots are kept to go back to.
pub const KEPT_PLOTS: usize = 12;
/// How many entries the console keeps; the oldest go first.
pub const KEPT_ENTRIES: usize = 300;
/// How many lines typed are kept, to step back through.
pub const KEPT_HISTORY: usize = 500;

/// The console's language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    Python,
    R,
}

impl Lang {
    pub fn name(self) -> &'static str {
        match self {
            Lang::Python => "Python",
            Lang::R => "R",
        }
    }

    /// As kernels name their language.
    fn kernel_language(self) -> &'static str {
        match self {
            Lang::Python => "python",
            Lang::R => "R",
        }
    }

    /// The prompt shown before what's typed, as each language's own console has it.
    pub fn prompt(self) -> &'static str {
        match self {
            Lang::Python => ">>>",
            Lang::R => ">",
        }
    }

    pub fn from_style(style: crate::code_cells::Style) -> Lang {
        match style {
            crate::code_cells::Style::Python => Lang::Python,
            crate::code_cells::Style::R => Lang::R,
        }
    }
}

/// The kernel for a language: the first installed whose language it is.
pub fn pick_spec(specs: &[KernelSpec], lang: Lang) -> Option<KernelSpec> {
    specs.iter().find(|s| s.language.eq_ignore_ascii_case(lang.kernel_language())).cloned()
}

pub struct Console {
    pub lang: Lang,
    pub session: Session,
    /// Everything sent, with what came back, as a notebook's cells.
    pub book: Notebook,
    /// What's been typed, oldest first.
    pub history: Vec<String>,
    /// While stepping back through the history: where, and what was being typed before.
    browsing: Option<usize>,
    saved_draft: String,
    /// What's being typed.
    pub draft: String,
    /// The pictures drawn (nbformat outputs), oldest first, and the one shown.
    pub plots: Vec<Value>,
    pub plot_at: usize,
    /// Entries whose pictures have been taken into `plots`.
    harvested: HashSet<u64>,
    /// Something to tell the person about the kernel (the environment has no ipykernel).
    pub note: Option<String>,
}

impl Console {
    pub fn new(lang: Lang, kernels: Kernels, folder: &Path) -> Self {
        let mut book = Notebook::new(if lang == Lang::R { "ir" } else { "python3" }, lang.name(), lang.kernel_language());
        book.cells.clear();
        Console { lang, session: Session::new(kernels, folder), book, history: Vec::new(), browsing: None, saved_draft: String::new(), draft: String::new(), plots: Vec::new(), plot_at: 0, harvested: HashSet::new(), note: None }
    }

    /// For tests: a console on a kernel already made.
    #[cfg(test)]
    pub fn with_session(lang: Lang, session: Session) -> Self {
        let mut console = Console::new(lang, crate::notebook_run::Kernels::fake(), Path::new("."));
        console.session = session;
        console
    }

    pub fn start(&mut self, spec: KernelSpec) {
        self.session.start(spec);
    }

    /// Whether it has a kernel, or one starting.
    pub fn has_kernel(&self) -> bool {
        self.session.has_kernel()
    }

    pub fn status(&self) -> KernelStatus {
        self.session.status
    }

    /// Runs code, as a new entry. Returns false when there's no kernel to run it on (or nothing to run).
    pub fn send(&mut self, code: &str) -> bool {
        let code = code.trim_end();
        if code.trim().is_empty() {
            return false;
        }
        self.session.revive();
        if !self.session.has_kernel() {
            return false;
        }
        let at = self.book.cells.len();
        self.book.insert_cell(at, CellKind::Code);
        self.book.cells[at].source = code.to_string();
        let uid = self.book.cells[at].uid;
        self.remember(code);
        self.forget_oldest();
        self.session.run(&mut self.book, &[uid])
    }

    /// Runs what's been typed.
    pub fn send_draft(&mut self) -> bool {
        let code = std::mem::take(&mut self.draft);
        let sent = self.send(&code);
        if !sent {
            self.draft = code;
        }
        sent
    }

    fn remember(&mut self, code: &str) {
        self.browsing = None;
        if self.history.last().is_none_or(|last| last != code) {
            self.history.push(code.to_string());
        }
        let extra = self.history.len().saturating_sub(KEPT_HISTORY);
        self.history.drain(..extra);
    }

    /// The oldest entries go once there are too many, but not one still running.
    fn forget_oldest(&mut self) {
        while self.book.cells.len() > KEPT_ENTRIES && self.session.state_of(self.book.cells[0].uid) == CellState::Idle {
            self.harvested.remove(&self.book.cells[0].uid);
            self.book.cells.remove(0);
        }
    }

    /// Up: what was typed before this. The draft is kept, to come back to.
    pub fn history_back(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let at = match self.browsing {
            None => {
                self.saved_draft = self.draft.clone();
                self.history.len() - 1
            }
            Some(at) => at.saturating_sub(1),
        };
        self.browsing = Some(at);
        self.draft = self.history[at].clone();
    }

    /// Down: what was typed after this, and at the end, the draft as it was.
    pub fn history_forward(&mut self) {
        let Some(at) = self.browsing else { return };
        if at + 1 < self.history.len() {
            self.browsing = Some(at + 1);
            self.draft = self.history[at + 1].clone();
        } else {
            self.browsing = None;
            self.draft = std::mem::take(&mut self.saved_draft);
        }
    }

    /// Takes in what the kernel has said. Returns whether anything changed, to draw again.
    pub fn pump(&mut self) -> bool {
        let changed = self.session.pump(&mut self.book);
        if changed {
            self.harvest();
        }
        changed
    }

    /// The pictures of entries that have finished, kept to go back to; the newest is shown.
    fn harvest(&mut self) {
        let mut added = false;
        for cell in &self.book.cells {
            if self.harvested.contains(&cell.uid) || self.session.state_of(cell.uid) != CellState::Idle || cell.execution_count.is_none() {
                continue;
            }
            self.harvested.insert(cell.uid);
            for out in &cell.outputs {
                if is_picture(out) {
                    self.plots.push(out.clone());
                    added = true;
                }
            }
        }
        let extra = self.plots.len().saturating_sub(KEPT_PLOTS);
        self.plots.drain(..extra);
        if added {
            self.plot_at = self.plots.len().saturating_sub(1);
        }
        self.plot_at = self.plot_at.min(self.plots.len().saturating_sub(1));
    }

    pub fn plot_back(&mut self) {
        self.plot_at = self.plot_at.saturating_sub(1);
    }

    pub fn plot_forward(&mut self) {
        if self.plot_at + 1 < self.plots.len() {
            self.plot_at += 1;
        }
    }

    /// The plot shown, if there are any.
    pub fn plot(&self) -> Option<&Value> {
        self.plots.get(self.plot_at)
    }

    /// Clears what's shown; the kernel's variables and the history stay.
    pub fn clear(&mut self) {
        let keep: Vec<_> = self.book.cells.iter().filter(|c| self.session.state_of(c.uid) != CellState::Idle).cloned().collect();
        self.book.cells = keep;
        self.harvested.clear();
    }

    pub fn clear_plots(&mut self) {
        self.plots.clear();
        self.plot_at = 0;
    }

    pub fn interrupt(&mut self) {
        self.session.interrupt();
    }

    pub fn restart(&mut self) {
        self.session.restart();
    }

    /// The whole transcript as plain text, for copying: each entry's code after its prompt, then what it printed.
    pub fn transcript(&self) -> String {
        let mut text = String::new();
        for cell in &self.book.cells {
            for (n, line) in cell.source.lines().enumerate() {
                text.push_str(if n == 0 { self.lang.prompt() } else { "..." });
                text.push(' ');
                text.push_str(line);
                text.push('\n');
            }
            for out in &cell.outputs {
                if !is_picture(out) {
                    let words = output::plain_text_of(out);
                    if !words.trim().is_empty() {
                        text.push_str(words.trim_end());
                        text.push('\n');
                    }
                }
            }
        }
        text
    }
}

/// Whether an output is a picture (a plot) rather than text.
pub fn is_picture(out: &Value) -> bool {
    matches!(output::choose(out), Shown::Png(_) | Shown::Jpeg(_) | Shown::Svg(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jupyter::link::KernelEvent;
    use crate::notebook_run::tests::scripted;
    use serde_json::json;

    /// A tiny PNG, one pixel, as a kernel would send a plot.
    const PIXEL: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    #[test]
    fn code_sent_runs_and_its_output_comes_back() {
        let mut console = Console::new(Lang::Python, Kernels::fake(), Path::new("."));
        assert!(!console.send("1 + 1"), "no kernel yet");
        console.start(pick_spec(&(Kernels::fake().list)(), Lang::Python).unwrap());
        let began = std::time::Instant::now();
        while !console.send("1 + 1") && began.elapsed().as_secs() < 5 {
            console.pump();
        }
        while console.book.cells[0].outputs.is_empty() && began.elapsed().as_secs() < 5 {
            console.pump();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(console.transcript().contains(">>> 1 + 1\n"));
        assert!(console.transcript().contains("Here is the code that was given"), "{}", console.transcript());
        assert_eq!(pick_spec(&(Kernels::fake().list)(), Lang::R).unwrap().name, "ir");
    }

    #[test]
    fn the_history_steps_back_and_forward_keeping_the_draft() {
        let (session, _) = scripted();
        let mut console = Console::with_session(Lang::R, session);
        for code in ["x <- 1", "y <- 2", "y <- 2", "x + y"] {
            console.draft = code.into();
            assert!(console.send_draft());
            assert!(console.draft.is_empty());
        }
        assert_eq!(console.history, ["x <- 1", "y <- 2", "x + y"], "the same twice running is kept once");
        console.draft = "half typ".into();
        console.history_back();
        assert_eq!(console.draft, "x + y");
        console.history_back();
        console.history_back();
        console.history_back();
        assert_eq!(console.draft, "x <- 1", "it stops at the oldest");
        console.history_forward();
        assert_eq!(console.draft, "y <- 2");
        console.history_forward();
        console.history_forward();
        assert_eq!(console.draft, "half typ", "past the newest, the draft comes back");
        assert!(!console.send("   "), "nothing to run");
    }

    #[test]
    fn plots_are_kept_to_go_back_to() {
        let (session, shared) = scripted();
        let mut console = Console::with_session(Lang::Python, session);
        let picture = json!({"output_type": "display_data", "data": {"image/png": PIXEL, "text/plain": "<Figure>"}, "metadata": {}});
        for n in 0..(KEPT_PLOTS + 2) {
            assert!(console.send(&format!("plot({n})")));
            let request = shared.lock().unwrap().executed.len() as u64;
            shared.lock().unwrap().events.extend([
                KernelEvent::Output { request, output: picture.clone() },
                KernelEvent::Output { request, output: json!({"output_type": "stream", "name": "stdout", "text": format!("drawn {n}\n")}) },
                KernelEvent::Done { request, execution_count: Some(n as u32 + 1), ok: true },
            ]);
            console.pump();
        }
        assert_eq!(console.plots.len(), KEPT_PLOTS, "only the last few are kept");
        assert_eq!(console.plot_at, KEPT_PLOTS - 1, "the newest is shown");
        console.plot_back();
        console.plot_back();
        assert_eq!(console.plot_at, KEPT_PLOTS - 3);
        console.plot_forward();
        assert_eq!(console.plot_at, KEPT_PLOTS - 2);
        assert!(console.transcript().contains("drawn 13"));
        assert!(!console.transcript().contains("<Figure>"), "pictures aren't in the text");
        console.clear();
        assert!(console.book.cells.is_empty() && console.plots.len() == KEPT_PLOTS, "clearing keeps the plots");
    }

    /// The real kernels, where they're installed: Python's ipykernel and R's IRkernel.
    #[test]
    fn the_real_kernels_answer_the_console() {
        let kernels = Kernels { start: crate::jupyter::start, list: crate::jupyter::kernel_specs };
        let specs = (kernels.list)();
        for (lang, code, wanted) in [(Lang::Python, "print(6 * 7)", "42"), (Lang::R, "cat(6 * 7)", "42")] {
            let Some(spec) = pick_spec(&specs, lang) else {
                eprintln!("No {} kernel installed: the console wasn't tried with one.", lang.name());
                continue;
            };
            let mut console = Console::new(lang, kernels, &std::env::temp_dir());
            console.start(spec);
            let began = std::time::Instant::now();
            let mut sent = false;
            while began.elapsed().as_secs() < 60 {
                console.pump();
                if !sent && console.status() == KernelStatus::Idle {
                    sent = console.send(code);
                }
                if console.transcript().contains(wanted) || console.session.problem.is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            assert!(console.transcript().contains(wanted), "{}: {}\n{:?}", lang.name(), console.transcript(), console.session.problem);
        }
    }
}
