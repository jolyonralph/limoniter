//! A Jupyter notebook (`.ipynb`) in its tab: cells one under another, each code cell edited in place with the
//! language's colours, Markdown shown as it reads, and what running left below each. The file's text stays the
//! document underneath, as in table view: every change to the notebook is written back into it as one undoable
//! edit, so saving, Undo, recovery and every encoding work as they always do. If the text is changed some other
//! way (Undo, a reload, editing it as JSON) the notebook is read again from it.
//!
//! Which kernel runs the code, the queue of cells and what the kernel's events do are in `notebook_run`; what
//! outputs are made into is in `notebook_output`; the file itself is in `notebook`.

use super::*;
use crate::jupyter::link::{KernelSpec, KernelStatus};
use crate::notebook::{CellKind, Notebook, Problem};
use crate::data_table::DataTable;
use crate::notebook_output::{self as output, Shown};
use crate::notebook_run::{CellState, Kernels, ListKernels, Session};
use eframe::egui::text::{CCursor, CCursorRange, LayoutJob};
use eframe::egui::{Align, Color32, FontId, Id, Layout, Rect, Sense, TextFormat, vec2};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, TryRecvError};

/// How tall an output grows before it scrolls inside its cell.
const OUTPUT_MOST_HEIGHT: f32 = 320.0;
/// The width of the margin with each cell's `[n]`.
const GUTTER: f32 = 58.0;

/// What a notebook can be told to do, from its menu, its bar and its keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Act {
    /// Run the cell, then go to the next (adding one after the last).
    RunCell,
    /// Run the cell and stay on it.
    RunCellStay,
    /// Run the cell and put a new one below it.
    RunCellInsert,
    RunAll,
    /// Every cell above the chosen one.
    RunAbove,
    /// The chosen cell and every one below it.
    RunBelow,
    Interrupt,
    Restart,
    RestartRunAll,
    ClearOutputs,
    ClearCellOutput,
    /// Hide or show the chosen cell's outputs.
    ToggleOutput,
    InsertAbove,
    InsertBelow,
    Delete,
    MoveUp,
    MoveDown,
    /// Split the cell at the cursor.
    Split,
    /// Join the cell with the next.
    Join,
    ToCode,
    ToMarkdown,
    ToRaw,
    /// View, Notebook as text: the JSON, to read or edit.
    AsText,
    /// Compare the notebook with the last commit, cell by cell.
    CompareLast,
    /// The same, with each cell's outputs and count too.
    CompareLastWithOutputs,
    /// Look for the installed kernels again, after one has been installed.
    RescanKernels,
    /// Install a kernel for the notebook's language, in the terminal.
    InstallKernel,
}

/// The kernels that can be started, found on another thread: looking for them can mean asking Python.
struct SpecList {
    found: Option<Vec<KernelSpec>>,
    waiting: Option<Receiver<Vec<KernelSpec>>>,
}

impl SpecList {
    fn start(list: ListKernels, ctx: Option<eframe::egui::Context>) -> Self {
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(list());
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
        Self { found: None, waiting: Some(receive) }
    }

    fn get(&mut self) -> Option<&[KernelSpec]> {
        if let Some(receive) = &self.waiting {
            match receive.try_recv() {
                Ok(list) => {
                    self.found = Some(list);
                    self.waiting = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.found = Some(Vec::new());
                    self.waiting = None;
                }
            }
        }
        self.found.as_deref()
    }
}

/// How a request to run cells got on.
#[derive(Debug, PartialEq, Eq)]
enum Kernel {
    /// There is a kernel (or one is starting), and the cells are queued.
    Ready,
    /// The list of kernels isn't here yet: try again when it is.
    Waiting,
    /// None could be chosen; the notice says so.
    Missing,
}

/// A notebook tab's own state: the notebook, which cell is chosen, and the kernel running it.
pub(super) struct NotebookState {
    nb: Notebook,
    /// The text's version the notebook was last read from or written to.
    synced: u64,
    /// Why the text can't be shown as a notebook.
    problem: Option<Problem>,
    selected: usize,
    /// Whether a cell's editor had the keyboard when last drawn.
    editing: bool,
    /// Markdown cells shown as their text.
    source_shown: HashSet<u64>,
    session: Session,
    specs: SpecList,
    /// How the kernels are found, to look again.
    list: ListKernels,
    /// Cells to run once the list of kernels is here.
    run_after: Option<Vec<u64>>,
    /// The first key of a two-key command (D D, I I).
    pending_key: Option<Key>,
    scroll_to_selected: bool,
    /// Give this cell's editor the keyboard on the next frame.
    focus_cell: Option<u64>,
    answer: String,
    /// What was said about the last thing asked for, shown in the bar.
    notice: Option<String>,
    /// The cursor in the cell being edited, as a character's place: the cell and the place.
    caret: Option<(u64, usize)>,
    colours: HashMap<u64, (u64, Vec<(String, Color32)>)>,
    syntax: Option<(String, Option<usize>)>,
    markdown: HashMap<u64, (u64, Vec<preview::Block>)>,
    pictures: HashMap<(u64, usize), (u64, Option<eframe::egui::TextureHandle>)>,
    /// What the kernel holds, for the Variables panel.
    pub(super) vars: Variables,
}

/// The kernel's variables as the Variables panel shows them, kept up to date by asking after each run.
#[derive(Default)]
pub(super) struct Variables {
    pub(super) list: Vec<crate::notebook_vars::Var>,
    /// Whether the panel is open, so there's a reason to ask.
    pub(super) watching: bool,
    /// The listing asked for, which hasn't come back yet.
    listing: Option<u64>,
    /// Variables asked for to open in the data viewer: the request, and the variable's name.
    fetching: Vec<(u64, String)>,
    /// Whether something has run since the list was made.
    stale: bool,
    was_running: bool,
    /// Why the last listing or opening failed.
    pub(super) problem: Option<String>,
    /// Tables that have come, for the window to open in the data viewer.
    pub(super) ready: Vec<(String, DataTable)>,
}

fn hash_of(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn editor_id(uid: u64) -> Id {
    views::notebook_id().with(("cell", uid))
}

impl NotebookState {
    pub(super) fn open(text: &str, kernels: Kernels, folder: &Path, version: u64, ctx: Option<eframe::egui::Context>) -> Self {
        let (nb, problem) = match Notebook::parse(text) {
            Ok(nb) => (nb, None),
            Err(problem) => (Notebook::new("python3", "Python 3", "python"), Some(problem)),
        };
        let mut session = Session::new(kernels, folder);
        if let Some(ctx) = &ctx {
            session.set_repaint(ctx);
        }
        Self {
            nb,
            synced: version,
            problem,
            selected: 0,
            editing: false,
            source_shown: HashSet::new(),
            session,
            specs: SpecList::start(kernels.list, ctx),
            list: kernels.list,
            run_after: None,
            pending_key: None,
            scroll_to_selected: false,
            focus_cell: None,
            answer: String::new(),
            notice: None,
            caret: None,
            colours: HashMap::new(),
            syntax: None,
            markdown: HashMap::new(),
            pictures: HashMap::new(),
            vars: Variables { stale: true, ..Variables::default() },
        }
    }

    pub(super) fn problem(&self) -> Option<&Problem> {
        self.problem.as_ref()
    }

    /// Reads the notebook again from the text, after the text was changed some other way.
    fn reread(&mut self, text: &str, version: u64) {
        self.synced = version;
        match Notebook::parse(text) {
            Ok(mut nb) => {
                nb.keep_uids_from(&self.nb);
                self.nb = nb;
                self.problem = None;
                self.selected = self.selected.min(self.nb.cells.len().saturating_sub(1));
            }
            Err(problem) => self.problem = Some(problem),
        }
    }

    /// Writes the notebook into the text as one undoable edit.
    fn write_into(&mut self, text: &mut TextBuffer) {
        let rows = self.nb.to_lines();
        let last = text.line_count().saturating_sub(1);
        text.replace_lines(0, last, rows);
        self.synced = text.version();
    }

    #[cfg(test)]
    pub(super) fn cells(&self) -> &[crate::notebook::Cell] {
        &self.nb.cells
    }

    #[cfg(test)]
    pub(super) fn notebook(&self) -> &Notebook {
        &self.nb
    }

    /// The cell that's chosen, by its place.
    #[cfg(test)]
    pub(super) fn chosen(&self) -> usize {
        self.selected
    }

    #[cfg(test)]
    pub(super) fn set_source(&mut self, at: usize, source: &str) {
        self.nb.cells[at].source = source.to_string();
    }

    fn selected_uid(&self) -> Option<u64> {
        self.nb.cells.get(self.selected).map(|c| c.uid)
    }

    pub(super) fn select(&mut self, at: usize) {
        self.selected = at.min(self.nb.cells.len().saturating_sub(1));
        self.pending_key = None;
        self.scroll_to_selected = true;
    }

    /// Finds a kernel for the notebook and starts it if there isn't one.
    fn ensure_kernel(&mut self) -> Kernel {
        if self.session.has_kernel() {
            self.session.revive();
            return Kernel::Ready;
        }
        let Some(specs) = self.specs.get() else {
            return Kernel::Waiting;
        };
        match Session::pick_spec(specs, &self.nb) {
            Some(spec) => {
                self.session.start(spec);
                Kernel::Ready
            }
            None => {
                self.notice = Some(crate::i18n::tr("No kernel is chosen for this notebook. Choose one from the Kernel menu above.").to_string());
                Kernel::Missing
            }
        }
    }

    /// Queues cells to run; code is the only thing that runs, and Markdown is shown as it reads.
    fn run_cells(&mut self, uids: Vec<u64>) {
        for uid in &uids {
            self.source_shown.remove(uid);
        }
        let has_code = uids.iter().any(|uid| self.nb.index_of(*uid).is_some_and(|at| self.nb.cells[at].kind == CellKind::Code));
        if !has_code {
            return;
        }
        self.notice = None;
        match self.ensure_kernel() {
            Kernel::Ready => {
                self.session.run(&mut self.nb, &uids);
            }
            Kernel::Waiting => self.run_after = Some(uids),
            Kernel::Missing => {}
        }
    }

    /// Choosing a kernel from the menu: the notebook says which it uses, and the one running is replaced.
    fn use_kernel(&mut self, spec: KernelSpec) {
        self.nb.set_kernel(&spec.name, &spec.display_name, &spec.language);
        self.notice = None;
        self.session.start(spec);
    }

    /// Once a frame: what the kernel has said, and a run that was waiting for the list of kernels. Returns whether
    /// the notebook changed.
    fn tick(&mut self) -> bool {
        if self.run_after.is_some() && self.specs.get().is_some() {
            let uids = self.run_after.take().unwrap_or_default();
            self.run_cells(uids);
        }
        let changed = self.session.pump(&mut self.nb);
        if self.session.input.is_none() {
            self.answer.clear();
        }
        self.refresh_variables();
        changed
    }

    /// The language the kernel runs, as the Variables panel needs it.
    fn kernel_language(&self) -> String {
        self.session.language().unwrap_or_else(|| self.language())
    }

    /// Keeps the list of variables up to date: asked again after the cells have run, if the panel is open, and
    /// what the kernel said is taken in.
    fn refresh_variables(&mut self) {
        let running = self.session.is_running_anything();
        if self.vars.was_running && !running {
            self.vars.stale = true;
        }
        self.vars.was_running = running;
        for (token, answer) in self.session.take_answers() {
            if self.vars.listing == Some(token) {
                self.vars.listing = None;
                match answer.and_then(|text| crate::notebook_vars::parse_list(&text)) {
                    Ok(list) => {
                        self.vars.list = list;
                        self.vars.problem = None;
                    }
                    Err(why) => self.vars.problem = Some(why),
                }
            } else if let Some(at) = self.vars.fetching.iter().position(|(t, _)| *t == token) {
                let (_, name) = self.vars.fetching.remove(at);
                match answer.and_then(|text| crate::notebook_vars::parse_table(&text)) {
                    Ok(table) => self.vars.ready.push((name, table)),
                    Err(why) => self.vars.problem = Some(crate::i18n::tf!("{name} couldn't be opened: {why}")),
                }
            }
        }
        // A kernel that has been stopped or restarted holds none of what was listed.
        if matches!(self.session.status, KernelStatus::Dead | KernelStatus::Starting) && !self.vars.list.is_empty() {
            self.vars.list.clear();
            self.vars.stale = true;
        }
        if self.vars.watching
            && self.vars.stale
            && self.vars.listing.is_none()
            && self.session.has_kernel()
            && self.session.status != KernelStatus::Dead
            && let Some(code) = crate::notebook_vars::list_code(&self.kernel_language())
        {
            self.vars.listing = Some(self.session.ask(code));
            self.vars.stale = false;
        }
    }

    /// Asks the kernel for the first rows of a variable, to open in the data viewer when they come.
    pub(super) fn open_variable(&mut self, name: &str) {
        if self.vars.fetching.iter().any(|(_, n)| n == name) {
            return;
        }
        if let Some(code) = crate::notebook_vars::fetch_code(&self.kernel_language(), name, crate::notebook_vars::MOST_ROWS) {
            let token = self.session.ask(code);
            self.vars.fetching.push((token, name.to_string()));
            self.vars.problem = None;
        }
    }

    /// The Variables panel's contents. Returns nothing; a double-click on a table asks for it.
    pub(super) fn draw_variables(&mut self, ui: &mut Ui) {
        let language = self.kernel_language();
        if !crate::notebook_vars::can_list(&language) {
            ui.label(RichText::new(crate::i18n::tf!("Variables can be listed for Python and R kernels. This notebook's kernel runs {language}.")).color(theme::text_dim()));
            return;
        }
        if !self.session.has_kernel() {
            ui.label(RichText::new(crate::i18n::tr("Run a cell to start the kernel; its variables are listed here after each run.")).color(theme::text_dim()));
            return;
        }
        ui.horizontal(|ui| {
            if ui.small_button(crate::i18n::tr("Refresh")).on_hover_text(crate::i18n::tr("Ask the kernel again what it holds")).clicked() {
                self.vars.stale = true;
            }
            let working = self.vars.listing.is_some() || !self.vars.fetching.is_empty();
            if working {
                ui.label(RichText::new(crate::i18n::tr("Asking the kernel\u{2026}")).color(theme::text_dim()));
            }
        });
        if let Some(problem) = &self.vars.problem {
            ui.label(RichText::new(problem).color(theme::problem_mark()));
        }
        if self.vars.list.is_empty() {
            ui.label(RichText::new(crate::i18n::tr("The kernel holds no variables yet.")).color(theme::text_dim()));
            return;
        }
        let mut open = None;
        eframe::egui::ScrollArea::both().id_salt("variables-scroll").auto_shrink([false, false]).show(ui, |ui| {
            eframe::egui::Grid::new("variables-grid").striped(true).num_columns(4).spacing([14.0, 4.0]).show(ui, |ui| {
                for head in [crate::i18n::tr("Name"), crate::i18n::tr("Type"), crate::i18n::tr("Size"), crate::i18n::tr("Value")] {
                    ui.label(RichText::new(head).color(theme::text_dim()));
                }
                ui.end_row();
                for var in &self.vars.list {
                    let opens = var.shape.opens();
                    let hover = if opens { crate::i18n::tr("Double-click to open it in the data viewer") } else { crate::i18n::tr("Only tables can be opened in the data viewer") };
                    let name = ui.add(eframe::egui::Label::new(RichText::new(&var.name).strong()).sense(Sense::click())).on_hover_text(hover);
                    let kind = ui.add(eframe::egui::Label::new(&var.type_name).sense(Sense::click()));
                    let size = ui.add(eframe::egui::Label::new(&var.size).sense(Sense::click()));
                    let value = ui.add(eframe::egui::Label::new(&var.value).truncate().sense(Sense::click()));
                    ui.end_row();
                    if opens && [name, kind, size, value].iter().any(|r| r.double_clicked()) {
                        open = Some(var.name.clone());
                    }
                }
            });
        });
        if let Some(name) = open {
            self.open_variable(&name);
        }
    }

    /// Whether something is going on that needs the window drawn again soon.
    pub(super) fn busy(&self) -> bool {
        self.session.is_running_anything() || self.session.is_asking() || self.session.status == KernelStatus::Starting || self.session.has_kernel() && self.session.input.is_some() || self.run_after.is_some()
    }

    /// The notebook's language, as its kernel would be found by ("python" when it doesn't say).
    pub(super) fn language(&self) -> String {
        self.nb.language().filter(|l| !l.is_empty()).unwrap_or("python").to_string()
    }

    /// Says a kernel is being installed in the terminal, and what to do after.
    pub(super) fn installing(&mut self) {
        self.notice = Some(crate::i18n::tr("A kernel is being installed in the terminal. When it has finished, choose Look for kernels again from the Kernel menu.").to_string());
    }

    /// Carries out a command, returning whether the notebook changed.
    pub(super) fn act(&mut self, act: Act, read_only: bool) -> bool {
        let mutates = !matches!(act, Act::Interrupt | Act::AsText | Act::CompareLast | Act::CompareLastWithOutputs | Act::RescanKernels | Act::InstallKernel);
        if read_only && mutates && !matches!(act, Act::RunCell | Act::RunCellStay | Act::RunAll | Act::RunAbove | Act::RunBelow | Act::Restart | Act::RestartRunAll) {
            return false;
        }
        let at = self.selected.min(self.nb.cells.len().saturating_sub(1));
        let all: Vec<u64> = self.nb.cells.iter().map(|c| c.uid).collect();
        match act {
            Act::RunCell | Act::RunCellStay | Act::RunCellInsert => {
                let Some(uid) = self.selected_uid() else { return false };
                self.run_cells(vec![uid]);
                if read_only {
                    return false;
                }
                let last = at + 1 >= self.nb.cells.len();
                match act {
                    Act::RunCellStay => true,
                    Act::RunCellInsert => {
                        let to = self.nb.insert_cell(at + 1, CellKind::Code);
                        self.select(to);
                        true
                    }
                    _ if last => {
                        let to = self.nb.insert_cell(at + 1, CellKind::Code);
                        self.select(to);
                        true
                    }
                    _ => {
                        self.select(at + 1);
                        true
                    }
                }
            }
            // Cells that are run lose what they showed before, so these change the notebook.
            Act::RunAll => {
                self.run_cells(all);
                true
            }
            Act::RunAbove => {
                self.run_cells(all[..at].to_vec());
                true
            }
            Act::RunBelow => {
                self.run_cells(all[at..].to_vec());
                true
            }
            Act::Interrupt => {
                self.session.interrupt();
                false
            }
            Act::Restart => {
                self.session.restart();
                false
            }
            Act::RestartRunAll => {
                self.session.restart();
                self.nb.clear_all_outputs();
                self.run_cells(all);
                true
            }
            Act::ClearOutputs => self.nb.clear_all_outputs(),
            Act::ClearCellOutput => match self.nb.cells.get_mut(at) {
                Some(cell) if !cell.outputs.is_empty() || cell.execution_count.is_some() => {
                    cell.clear_outputs();
                    true
                }
                _ => false,
            },
            Act::ToggleOutput => match self.nb.cells.get_mut(at) {
                Some(cell) if cell.kind == CellKind::Code => {
                    let hidden = cell.outputs_hidden();
                    cell.set_outputs_hidden(!hidden);
                    true
                }
                _ => false,
            },
            Act::InsertAbove => {
                let to = self.nb.insert_cell(at, CellKind::Code);
                self.select(to);
                true
            }
            Act::InsertBelow => {
                let to = self.nb.insert_cell(if self.nb.cells.is_empty() { 0 } else { at + 1 }, CellKind::Code);
                self.select(to);
                true
            }
            Act::Delete => {
                let was_last = at + 1 >= self.nb.cells.len();
                let kept_one = self.nb.cells.len() == 1;
                if self.nb.delete_cell(at).is_none() {
                    return false;
                }
                if !kept_one && was_last {
                    self.select(at.saturating_sub(1));
                }
                true
            }
            Act::MoveUp | Act::MoveDown => match self.nb.move_cell(at, act == Act::MoveDown) {
                Some(to) => {
                    self.select(to);
                    true
                }
                None => false,
            },
            Act::Split => {
                let Some(cell) = self.nb.cells.get(at) else { return false };
                let offset = match self.caret {
                    Some((uid, index)) if uid == cell.uid => index,
                    _ => cell.source.chars().count(),
                };
                match self.nb.split_cell(at, offset) {
                    Some(to) => {
                        self.select(to);
                        true
                    }
                    None => false,
                }
            }
            Act::Join => self.nb.join_with_next(at),
            Act::ToCode | Act::ToMarkdown | Act::ToRaw => {
                let kind = match act {
                    Act::ToCode => CellKind::Code,
                    Act::ToMarkdown => CellKind::Markdown,
                    _ => CellKind::Raw,
                };
                if let Some(uid) = self.selected_uid() {
                    self.source_shown.remove(&uid);
                }
                self.nb.set_kind(at, kind)
            }
            Act::RescanKernels => {
                self.specs = SpecList::start(self.list, None);
                self.notice = None;
                false
            }
            Act::AsText | Act::CompareLast | Act::CompareLastWithOutputs | Act::InstallKernel => false,
        }
    }

    /// The command-mode keys of Jupyter: A and B add cells, D D deletes, M, Y and R change the kind, O hides the
    /// outputs, I I interrupts, the arrows (and J and K) move, and Enter edits the cell.
    pub(super) fn command_key(&mut self, key: Key, shift: bool) -> Option<Act> {
        let pending = self.pending_key.take();
        let twice = pending == Some(key);
        if shift {
            return None;
        }
        match key {
            Key::A => Some(Act::InsertAbove),
            Key::B => Some(Act::InsertBelow),
            Key::M => Some(Act::ToMarkdown),
            Key::Y => Some(Act::ToCode),
            Key::R => Some(Act::ToRaw),
            Key::O => Some(Act::ToggleOutput),
            Key::D if twice => Some(Act::Delete),
            Key::I if twice => Some(Act::Interrupt),
            Key::D | Key::I => {
                self.pending_key = Some(key);
                None
            }
            Key::ArrowUp | Key::K => {
                self.select(self.selected.saturating_sub(1));
                None
            }
            Key::ArrowDown | Key::J => {
                self.select(self.selected + 1);
                None
            }
            Key::Enter => {
                self.focus_cell = self.selected_uid();
                if let Some(uid) = self.focus_cell
                    && self.nb.cells[self.selected].kind == CellKind::Markdown
                {
                    self.source_shown.insert(uid);
                }
                None
            }
            _ => None,
        }
    }
}


/// The words and colour for the kernel's state, for the bar.
fn status_of(session: &Session) -> (String, Color32) {
    let (word, colour) = match session.status {
        _ if !session.has_kernel() && session.problem.is_none() => ("Not started", theme::text_faint()),
        KernelStatus::Idle => ("Idle", theme::git_added()),
        KernelStatus::Busy => ("Busy", theme::stopped_mark()),
        KernelStatus::Starting => ("Starting", theme::stopped_mark()),
        KernelStatus::Dead => ("Stopped", theme::problem_mark()),
    };
    (crate::i18n::tr(word).to_string(), colour)
}

/// What the notebook is coloured as, by the language its notebook names.
fn syntax_for(language: &str) -> Option<usize> {
    let engine = highlight::engine()?;
    let wanted = match language.to_ascii_lowercase().as_str() {
        "ipython" | "ipython3" | "python3" => "python".to_string(),
        "node" | "nodejs" | "javascript (node.js)" => "javascript".to_string(),
        other => other.to_string(),
    };
    engine.names().iter().find(|(name, _)| name.eq_ignore_ascii_case(&wanted)).map(|(_, index)| *index)
}

/// The text cut into stretches by colour, for the language's syntax.
fn colour_pieces(syntax: Option<usize>, text: &str) -> Vec<(String, Color32)> {
    let plain = theme::editor_text();
    let (Some(engine), Some(syntax)) = (highlight::engine(), syntax) else {
        return vec![(text.to_string(), plain)];
    };
    let rows: Vec<String> = text.split('\n').map(str::to_string).collect();
    let count = rows.len();
    let mut buffer = TextBuffer::default();
    buffer.set_content(rows.clone());
    let mut colours = highlight::LineColours::default();
    colours.set_syntax(Some(syntax));
    colours.prepare(engine, &mut buffer, 0, count, Duration::from_millis(40));
    let mut pieces = Vec::new();
    for (n, row) in rows.iter().enumerate() {
        let mut chars = row.chars();
        if let Some(line) = colours.line(n) {
            for (length, colour) in &line.spans {
                let piece: String = chars.by_ref().take(*length).collect();
                if !piece.is_empty() {
                    pieces.push((piece, *colour));
                }
            }
        }
        let rest: String = chars.collect();
        if !rest.is_empty() {
            pieces.push((rest, plain));
        }
        if n + 1 < count {
            pieces.push(("\n".to_string(), plain));
        }
    }
    pieces
}

/// A picture made into a texture, for pictures in outputs.
fn texture_of(ctx: &eframe::egui::Context, name: String, shown: &Shown) -> Option<eframe::egui::TextureHandle> {
    let picture = match shown {
        Shown::Png(bytes) => crate::pictures::decode(crate::pictures::Format::Png, bytes).ok()?,
        Shown::Jpeg(bytes) => crate::pictures::decode(crate::pictures::Format::Jpeg, bytes).ok()?,
        Shown::Svg(text) => preview::svg_picture(text.as_bytes(), None).ok()?,
        _ => return None,
    };
    let image = eframe::egui::ColorImage::from_rgba_unmultiplied([picture.width, picture.height], &picture.rgba);
    Some(ctx.load_texture(name, image, eframe::egui::TextureOptions::LINEAR))
}

/// Terminal-coloured text as a laid-out job, in the editor's font.
fn ansi_job(text: &str, font: &FontId, dark: bool, base: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    for run in output::ansi_runs(&output::settle_carriage_returns(text), dark) {
        let mut format = TextFormat::simple(font.clone(), run.colour.unwrap_or(base));
        if let Some(background) = run.background {
            format.background = background;
        }
        if run.underline {
            format.underline = eframe::egui::Stroke::new(1.0, run.colour.unwrap_or(base));
        }
        if run.bold {
            // Bold is shown by a brighter colour, as the editor has no bold monospace face.
            format.color = theme::mix(format.color, theme::strong_text(), 0.4);
        }
        job.append(&run.text, 0.0, format);
    }
    job
}

/// What the notebook's view needs from the window around it.
struct Look {
    font_size: f32,
    read_only: bool,
    dark: bool,
}

impl NotebookState {
    /// The notebook's bar: running, stopping, adding cells, and the kernel.
    fn draw_bar(&mut self, ui: &mut Ui, look: &Look) -> Vec<Act> {
        let mut acts = Vec::new();
        ui.horizontal_wrapped(|ui| {
            let mut button = |ui: &mut Ui, label: &str, hint: &str, act: Act| {
                if ui.button(crate::i18n::tr(label)).on_hover_text(crate::i18n::tr(hint)).clicked() {
                    acts.push(act);
                }
            };
            button(ui, "Run", "Run this cell and go to the next (Shift+Enter)", Act::RunCell);
            button(ui, "Stop", "Interrupt the kernel (I, I)", Act::Interrupt);
            button(ui, "Restart", "Start the kernel afresh: its variables are lost", Act::Restart);
            button(ui, "Run all", "Run every cell, from the top", Act::RunAll);
            ui.separator();
            if !look.read_only {
                button(ui, "+ Cell", "Add a cell below this one (B)", Act::InsertBelow);
                let kind = self.nb.cells.get(self.selected).map_or(CellKind::Code, |c| c.kind);
                let kind_label = match kind {
                    CellKind::Code => crate::i18n::tr("Code"),
                    CellKind::Markdown => crate::i18n::tr("Markdown"),
                    CellKind::Raw => crate::i18n::tr("Raw"),
                };
                crate::fit::menu_button(ui, kind_label.to_string(), |ui| {
                    for (label, act) in [("Code", Act::ToCode), ("Markdown", Act::ToMarkdown), ("Raw", Act::ToRaw)] {
                        if ui.button(crate::i18n::tr(label)).clicked() {
                            acts.push(act);
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text(crate::i18n::tr("What the chosen cell is"));
                ui.separator();
            }
            // The kernel: which, a way to change it, and how it is.
            let named = self.session.spec.as_ref().map(|s| s.display_name.clone()).or_else(|| self.nb.metadata.get("kernelspec").and_then(|k| k.get("display_name")).and_then(|n| n.as_str()).map(str::to_string)).unwrap_or_else(|| crate::i18n::tr("No kernel").to_string());
            let language = self.language();
            let mut chosen = None;
            crate::fit::menu_button(ui, crate::i18n::tf!("Kernel: {named}"), |ui| {
                match self.specs.get() {
                    None => {
                        ui.label(crate::i18n::tr("Looking for kernels\u{2026}"));
                    }
                    Some([]) => {
                        ui.label(crate::i18n::tr("No Jupyter kernels were found on this computer."));
                    }
                    Some(specs) => {
                        for (spec, label) in specs.iter().zip(crate::jupyter::specs::labels(specs)) {
                            if ui.button(label).clicked() {
                                chosen = Some(spec.clone());
                                ui.close();
                            }
                        }
                    }
                }
                if let Some(specs) = self.specs.get() {
                    if !specs.iter().any(|s| s.language.eq_ignore_ascii_case(&language)) && ui.button(crate::i18n::tf!("Install a kernel for {language}\u{2026}")).clicked() {
                        acts.push(Act::InstallKernel);
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr("Look for kernels again")).on_hover_text(crate::i18n::tr("After installing a kernel")).clicked() {
                        acts.push(Act::RescanKernels);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(crate::i18n::tr("Restart and run all cells")).clicked() {
                    acts.push(Act::RestartRunAll);
                    ui.close();
                }
            })
            .response
            .on_hover_text(crate::i18n::tr("Which kernel runs this notebook's code"));
            if let Some(spec) = chosen {
                self.use_kernel(spec);
            }
            let (word, colour) = status_of(&self.session);
            ui.label(RichText::new("\u{25CF}").color(colour));
            ui.label(RichText::new(word).color(theme::text_dim()));
            if let Some(problem) = self.session.problem.clone().or_else(|| self.notice.clone()) {
                ui.label(RichText::new(problem).color(theme::problem_mark())).on_hover_text(crate::i18n::tr("What went wrong with the kernel"));
            }
        });
        acts
    }

    /// Draws one cell: its margin, its editor or Markdown, and what running left. Returns what to do.
    fn draw_cell(&mut self, ui: &mut Ui, at: usize, look: &Look, acts: &mut Vec<Act>) -> bool {
        let uid = self.nb.cells[at].uid;
        let kind = self.nb.cells[at].kind;
        let chosen = self.selected == at;
        let state = self.session.state_of(uid);
        let font = FontId::monospace(look.font_size);
        let mut changed = false;
        let top = ui.cursor().min;
        let row = ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(GUTTER, 20.0), Layout::right_to_left(Align::TOP), |ui| {
                if kind == CellKind::Code {
                    let mark = match (state, self.nb.cells[at].execution_count) {
                        (CellState::Running | CellState::Queued, _) => "*".to_string(),
                        (CellState::Idle, Some(n)) => n.to_string(),
                        (CellState::Idle, None) => " ".to_string(),
                    };
                    ui.label(RichText::new(format!("[{mark}]")).monospace().color(if state == CellState::Idle { theme::text_faint() } else { theme::accent() }));
                }
            });
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 6.0).max(80.0));
                changed |= self.draw_source(ui, at, look, &font);
                if kind == CellKind::Code {
                    changed |= self.draw_outputs(ui, at, look, &font, acts);
                    self.draw_input_box(ui, uid);
                }
            });
        });
        let rect = Rect::from_min_max(top, row.response.rect.max).expand2(vec2(0.0, 2.0));
        // The bar at the left shows which cell is chosen, and whether it is being typed in.
        let editing_here = chosen && self.editing;
        if chosen {
            let colour = if editing_here { theme::git_added() } else { theme::accent() };
            ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(3.0, rect.height())), 0.0, colour);
        }
        if ui.input(|i| i.pointer.primary_pressed()) && ui.rect_contains_pointer(rect) && !chosen {
            self.select(at);
        }
        if chosen && self.scroll_to_selected {
            ui.scroll_to_rect(rect, None);
            self.scroll_to_selected = false;
        }
        ui.add_space(6.0);
        changed
    }

    /// A cell's own text: an editor for code and raw cells, Markdown as it reads until it's edited.
    fn draw_source(&mut self, ui: &mut Ui, at: usize, look: &Look, font: &FontId) -> bool {
        let uid = self.nb.cells[at].uid;
        let kind = self.nb.cells[at].kind;
        let chosen = self.selected == at;
        if kind == CellKind::Markdown && !self.source_shown.contains(&uid) {
            let source = self.nb.cells[at].source.clone();
            let key = hash_of(&source);
            if self.markdown.get(&uid).is_none_or(|(k, _)| *k != key) {
                self.markdown.insert(uid, (key, preview::render(preview::Kind::Markdown, &source)));
            }
            let frame = eframe::egui::Frame::new().inner_margin(eframe::egui::Margin::symmetric(6, 2));
            let shown = frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                if source.trim().is_empty() {
                    ui.label(RichText::new(crate::i18n::tr("Empty Markdown cell: double-click to write in it")).color(theme::text_faint()));
                } else if let Some((_, blocks)) = self.markdown.get(&uid) {
                    preview::draw_inline(ui, blocks);
                }
            });
            if !look.read_only && ui.interact(shown.response.rect, editor_id(uid).with("md"), Sense::click()).double_clicked() {
                self.select(at);
                self.source_shown.insert(uid);
                self.focus_cell = Some(uid);
            }
            return false;
        }
        let id = editor_id(uid);
        // Tab puts in spaces to the next stop of four, which is what Python code wants.
        let tab = chosen && self.editing && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab));
        if tab {
            let ctx = ui.ctx().clone();
            let mut state = eframe::egui::text_edit::TextEditState::load(&ctx, id).unwrap_or_default();
            let place = state.cursor.char_range().map_or(0usize, |r| r.primary.index.into());
            let source = &mut self.nb.cells[at].source;
            let byte = source.char_indices().nth(place).map_or(source.len(), |(b, _)| b);
            let column = source[..byte].rsplit('\n').next().map_or(0, |l| l.chars().count());
            let spaces = 4 - column % 4;
            source.insert_str(byte, &" ".repeat(spaces));
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(place + spaces))));
            state.store(&ctx, id);
        }
        let language = self.nb.language().map(str::to_string).or_else(|| self.session.language()).unwrap_or_else(|| "python".to_string());
        if self.syntax.as_ref().is_none_or(|(l, _)| *l != language) {
            self.syntax = Some((language.clone(), syntax_for(&language)));
        }
        let syntax = if kind == CellKind::Code { self.syntax.as_ref().and_then(|(_, s)| *s) } else { None };
        let colours = &mut self.colours;
        let font_for_layout = font.clone();
        let mut layouter = |ui: &Ui, text: &dyn eframe::egui::TextBuffer, wrap: f32| {
            let text = text.as_str();
            // Colours made before the grammars were loaded are plain, so the key says whether they were.
            let key = hash_of(text) ^ u64::from(crate::theme::is_light()) << 1 ^ u64::from(highlight::engine().is_some()) << 2 ^ (syntax.unwrap_or(0) as u64) << 8;
            if colours.get(&uid).is_none_or(|(k, _)| *k != key) {
                colours.insert(uid, (key, colour_pieces(syntax, text)));
            }
            let mut job = LayoutJob::default();
            job.wrap.max_width = wrap;
            if let Some((_, pieces)) = colours.get(&uid) {
                for (piece, colour) in pieces {
                    job.append(piece, 0.0, TextFormat::simple(font_for_layout.clone(), *colour));
                }
            }
            ui.painter().layout_job(job)
        };
        let source = &mut self.nb.cells[at].source;
        let edit = eframe::egui::TextEdit::multiline(source).id(id).font(font.clone()).lock_focus(true).desired_rows(1).desired_width(f32::INFINITY).interactive(!look.read_only).layouter(&mut layouter).show(ui);
        let response = edit.response;
        if self.focus_cell == Some(uid) {
            response.request_focus();
            self.focus_cell = None;
        }
        if response.gained_focus() && !chosen {
            self.select(at);
        }
        if response.has_focus() {
            self.selected = at;
            self.editing = true;
            self.caret = edit.cursor_range.map(|r| (uid, r.primary.index.into()));
        } else if chosen {
            self.editing = false;
            // A Markdown cell goes back to reading when it's left.
            if kind == CellKind::Markdown && !response.has_focus() && self.focus_cell.is_none() {
                self.source_shown.remove(&uid);
            }
        }
        response.changed()
    }

    fn draw_outputs(&mut self, ui: &mut Ui, at: usize, look: &Look, font: &FontId, acts: &mut Vec<Act>) -> bool {
        let uid = self.nb.cells[at].uid;
        if self.nb.cells[at].outputs.is_empty() {
            return false;
        }
        let hidden = self.nb.cells[at].outputs_hidden();
        let mut changed = false;
        ui.horizontal(|ui| {
            let count = self.nb.cells[at].outputs.len();
            if ui.small_button(crate::i18n::tr(if hidden { "Show output" } else { "Hide output" })).on_hover_text(crate::i18n::tr("Show or hide what this cell printed and drew (O)")).clicked() && !look.read_only {
                let cell = &mut self.nb.cells[at];
                cell.set_outputs_hidden(!hidden);
                changed = true;
            }
            if !look.read_only && ui.small_button(crate::i18n::tr("Clear output")).clicked() {
                self.selected = at;
                acts.push(Act::ClearCellOutput);
            }
            if hidden {
                ui.label(RichText::new(crate::i18n::tf!("{count} hidden")).color(theme::text_faint()));
            }
        });
        if hidden {
            return changed;
        }
        let outputs = self.nb.cells[at].outputs.clone();
        for (n, value) in outputs.iter().enumerate() {
            let shown = output::choose(value);
            let frame = eframe::egui::Frame::new().inner_margin(eframe::egui::Margin::symmetric(6, 2)).fill(theme::active_line().gamma_multiply(0.35));
            frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.draw_output(ui, uid, n, &shown, look, font);
            });
        }
        changed
    }

    fn draw_output(&mut self, ui: &mut Ui, uid: u64, n: usize, shown: &Shown, look: &Look, font: &FontId) {
        let base = theme::editor_text();
        match shown {
            Shown::Stream { name, text } => {
                let colour = if name == "stderr" { theme::problem_mark() } else { base };
                let job = ansi_job(text.trim_end_matches('\n'), font, look.dark, colour);
                scrolled(ui, (uid, n), |ui| {
                    ui.add(eframe::egui::Label::new(job).selectable(true));
                });
            }
            Shown::Text(text) => {
                let job = ansi_job(text, font, look.dark, base);
                scrolled(ui, (uid, n), |ui| {
                    ui.add(eframe::egui::Label::new(job).selectable(true));
                });
            }
            Shown::Error { name, message, traceback } => {
                if traceback.trim().is_empty() {
                    ui.label(RichText::new(format!("{name}: {message}")).monospace().color(theme::problem_mark()));
                } else {
                    let job = ansi_job(traceback, font, look.dark, base);
                    scrolled(ui, (uid, n), |ui| {
                    ui.add(eframe::egui::Label::new(job).selectable(true));
                });
                }
            }
            Shown::Png(_) | Shown::Jpeg(_) | Shown::Svg(_) => {
                let key = match shown {
                    Shown::Png(b) | Shown::Jpeg(b) => b.len() as u64 ^ hash_of(&format!("{:?}", &b[..b.len().min(64)])),
                    Shown::Svg(t) => hash_of(t),
                    _ => 0,
                };
                if self.pictures.get(&(uid, n)).is_none_or(|(k, _)| *k != key) {
                    let texture = texture_of(ui.ctx(), format!("notebook-{uid}-{n}"), shown);
                    self.pictures.insert((uid, n), (key, texture));
                }
                match self.pictures.get(&(uid, n)).and_then(|(_, t)| t.as_ref()) {
                    Some(texture) => {
                        let size = texture.size_vec2();
                        let scale = (ui.available_width() / size.x).min(1.0);
                        ui.add(eframe::egui::Image::new((texture.id(), size * scale)));
                    }
                    None => {
                        ui.label(RichText::new(crate::i18n::tr("(A picture that couldn't be drawn)")).color(theme::text_faint()));
                    }
                }
            }
            Shown::Markdown(text) => {
                let blocks = preview::render(preview::Kind::Markdown, text);
                preview::draw_inline(ui, &blocks);
            }
            Shown::Table { table, before_and_after } => {
                scrolled(ui, (uid, n), |ui| {
                    eframe::egui::Grid::new(("notebook-table", uid, n)).striped(true).spacing(vec2(14.0, 3.0)).show(ui, |ui| {
                        for row in &table.rows {
                            for (text, heading) in row {
                                let text = RichText::new(text).monospace();
                                ui.label(if *heading { text.strong() } else { text });
                            }
                            ui.end_row();
                        }
                    });
                });
                if !before_and_after.is_empty() {
                    ui.label(RichText::new(before_and_after).color(theme::text_dim()));
                }
            }
            Shown::Nothing(kinds) => {
                let what = kinds.join(", ");
                ui.label(RichText::new(if what.is_empty() { crate::i18n::tr("(Nothing to show)").to_string() } else { crate::i18n::tf!("(Output of a kind that can't be shown here: {what})") }).color(theme::text_faint()));
            }
        }
    }

    /// The box under a cell that has asked for typed input.
    fn draw_input_box(&mut self, ui: &mut Ui, uid: u64) {
        let Some(ask) = self.session.input.clone().filter(|a| a.uid == uid) else { return };
        let mut send = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new(&ask.prompt).monospace());
            let field = ui.add(eframe::egui::TextEdit::singleline(&mut self.answer).password(ask.password).desired_width(240.0).id(editor_id(uid).with("input")));
            if !field.has_focus() && !self.answer_touched() {
                field.request_focus();
            }
            send = (field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) || ui.button(crate::i18n::tr("Send")).clicked();
        });
        if send {
            let text = std::mem::take(&mut self.answer);
            self.session.answer(&text);
        }
    }

    fn answer_touched(&self) -> bool {
        !self.answer.is_empty()
    }
}

/// An output's text or table in a box that scrolls if it's tall or wide.
fn scrolled(ui: &mut Ui, salt: (u64, usize), add: impl FnOnce(&mut Ui)) {
    eframe::egui::ScrollArea::both().id_salt(("notebook-output", salt)).max_height(OUTPUT_MOST_HEIGHT).auto_shrink([false, true]).show(ui, |ui| {
        add(ui);
    });
}

impl Limoniter {
    /// Whether the tab in front is shown as a notebook.
    pub(super) fn notebook_shown(&self, index: usize) -> bool {
        let Some(doc) = self.docs.get(index) else { return false };
        !doc.placeholder && !doc.hex_mode && !doc.notebook_text && doc.browser.is_none() && doc.game.is_none() && doc.compare.is_none() && doc.merge.is_none() && doc.table.is_none() && doc.picture.is_none() && !doc.text.is_disk_backed() && doc.scanning.is_none() && is_notebook_name(&doc.path_or_name())
    }

    /// Draws the notebook of the tab in front.
    pub(super) fn notebook_view(&mut self, ui: &mut Ui, input_on: bool) {
        let font_size = self.view.font_size;
        let dark = !theme::is_light();
        let index = self.active;
        let watching = self.panels.variables;
        let kernels = crate::notebook_run::kernels();
        let ctx = ui.ctx().clone();
        let doc = &mut self.docs[index];
        let read_only = doc.text.read_only;
        let folder = doc.path.as_deref().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let version = doc.text.version();
        if doc.notebook.is_none() {
            let text = doc.text.all_lines().iter().map(|l| &**l).collect::<Vec<_>>().join("\n");
            doc.notebook = Some(Box::new(NotebookState::open(&text, kernels, &folder, version, Some(ctx.clone()))));
        }
        let Some(state) = doc.notebook.as_deref_mut() else { return };
        state.vars.watching = watching;
        if state.synced != version {
            let text = doc.text.all_lines().iter().map(|l| &**l).collect::<Vec<_>>().join("\n");
            state.reread(&text, version);
        }
        state.session.set_folder(&folder);
        let mut as_text = false;
        if let Some(problem) = state.problem().cloned() {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(problem.words()).color(theme::problem_mark()));
                ui.add_space(8.0);
                if ui.button(crate::i18n::tr("Show it as text")).clicked() {
                    as_text = true;
                }
            });
            if as_text {
                doc.notebook_text = true;
            }
            return;
        }
        let look = Look { font_size, read_only, dark };
        let mut changed = state.tick();
        if state.busy() {
            ctx.request_repaint_after(Duration::from_millis(60));
        }
        let mut acts: Vec<Act> = Vec::new();
        // Keys, before the cells draw: in a cell's editor they would otherwise be typed.
        if input_on {
            state.read_keys(&ctx, &mut acts);
        }
        acts.extend(state.draw_bar(ui, &look));
        ui.separator();
        let area = ui.available_rect_before_wrap();
        ui.interact(area, views::notebook_id(), Sense::focusable_noninteractive());
        let count = state.nb.cells.len();
        eframe::egui::ScrollArea::vertical().id_salt("notebook-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            for at in 0..count {
                changed |= state.draw_cell(ui, at, &look, &mut acts);
            }
            // Room to scroll the last cell to the middle, and a click here chooses nothing but takes the keys.
            ui.add_space(80.0);
        });
        if ui.input(|i| i.pointer.primary_pressed()) && ui.rect_contains_pointer(area) && !state.editing {
            ctx.memory_mut(|m| m.request_focus(views::notebook_id()));
        }
        for act in acts {
            if act == Act::AsText {
                as_text = true;
            } else {
                changed |= state.act(act, read_only);
            }
        }
        if changed && !read_only {
            state.write_into(&mut doc.text);
        }
        if as_text {
            doc.notebook_text = true;
        }
        // Tables asked for in the Variables panel that have come open in the data viewer.
        let name = doc.name();
        let tables = doc.notebook.as_deref_mut().map(|state| std::mem::take(&mut state.vars.ready)).unwrap_or_default();
        for (variable, table) in tables {
            self.open_data_table(&crate::i18n::tf!("{variable} from {name}"), table);
        }
    }

    /// The Variables panel, beside the notebook in front: what its kernel holds.
    pub(super) fn variables_panel(&mut self, ui: &mut Ui) {
        if !self.panels.variables {
            return;
        }
        let shown = self.notebook_shown(self.active);
        let state = self.docs.get_mut(self.active).and_then(|doc| doc.notebook.as_deref_mut()).filter(|_| shown);
        self.panels.show_variables(ui, |ui| match state {
            Some(state) => state.draw_variables(ui),
            None => {
                ui.label(RichText::new(crate::i18n::tr("Open a notebook, in front, to see its variables.")).color(theme::text_dim()));
            }
        });
    }

    /// A step of a picture of the notebook in front (see `screenshot_step`).
    pub(super) fn notebook_screenshot_step(&mut self, step: &str) {
        let act = match step {
            "run-all" => Act::RunAll,
            "run" => Act::RunCell,
            "text" => Act::AsText,
            "clear" => Act::ClearOutputs,
            "kind-markdown" => Act::ToMarkdown,
            "kind-code" => Act::ToCode,
            _ => {
                let (what, number) = step.split_once('-').unwrap_or((step, "0"));
                let number: usize = number.parse().unwrap_or(0);
                if let Some(state) = self.docs[self.active].notebook.as_deref_mut() {
                    state.select(number);
                    if what == "edit"
                        && let Some(uid) = state.selected_uid()
                    {
                        state.focus_cell = Some(uid);
                    }
                }
                return;
            }
        };
        self.notebook_command(act);
    }

    /// A command from a menu, the ribbon or the command box for the notebook in front.
    pub(super) fn notebook_command(&mut self, act: Act) {
        if !self.notebook_shown(self.active) && act != Act::AsText {
            return;
        }
        let index = self.active;
        if act == Act::AsText {
            let doc = &mut self.docs[index];
            doc.notebook_text = !doc.notebook_text;
            return;
        }
        if act == Act::InstallKernel {
            let Some(language) = self.docs[index].notebook.as_deref().map(NotebookState::language) else { return };
            match crate::jupyter::specs::install_command(&language) {
                Some((command, _)) => {
                    self.new_terminal(None);
                    if let Some(terminal) = self.terminal.as_mut() {
                        terminal.send(format!("{command}\r").as_bytes());
                    }
                    if let Some(state) = self.docs[index].notebook.as_deref_mut() {
                        state.installing();
                    }
                }
                None => self.message("Install a kernel", crate::i18n::tf!("Limoniter doesn't know how to install a Jupyter kernel for {language} here. For Python, install Python first; for R, install R. Other languages' kernels have their own instructions.")),
            }
            return;
        }
        if matches!(act, Act::CompareLast | Act::CompareLastWithOutputs) {
            if let Some(path) = self.docs[index].path.clone() {
                self.compare_notebook(&path, "HEAD", act == Act::CompareLastWithOutputs);
            } else {
                self.message("Compare with the last commit", crate::i18n::tr("Save the notebook first: it has to be in a Git repository to be compared.").into());
            }
            return;
        }
        let doc = &mut self.docs[index];
        let read_only = doc.text.read_only;
        let Some(state) = doc.notebook.as_deref_mut() else { return };
        if state.act(act, read_only) && !read_only {
            state.write_into(&mut doc.text);
        }
    }
}

impl Limoniter {
    /// Compares a notebook with how it was in a commit, cell by cell: each cell's source, with nothing about
    /// outputs or counts unless asked. Git's own line diff of the JSON is mostly outputs and noise.
    pub(super) fn compare_notebook(&mut self, path: &Path, revision: &str, outputs: bool) {
        let title = "Compare with the last commit";
        let Some(root) = crate::git::repository_of(path) else {
            return self.message(title, crate::i18n::tr("This file isn't in a Git repository.").into());
        };
        let Some(relative) = crate::git::relative(&root, path) else {
            return self.message(title, crate::i18n::tr("This file isn't in a Git repository.").into());
        };
        let Some(bytes) = crate::git::file_at(&root, revision, &relative) else {
            let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            return self.message(title, crate::i18n::tf!("{name} wasn't in that commit."));
        };
        // The notebook as it is now: in its tab, with any changes not saved, or else the file.
        let now = match self.docs.iter().find(|d| d.path.as_deref() == Some(path)) {
            Some(doc) => doc.text.all_lines().iter().map(|l| &**l).collect::<Vec<_>>().join("\n"),
            None => std::fs::read_to_string(path).unwrap_or_default(),
        };
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let lines = |json: &str| -> Vec<Arc<str>> { crate::notebook::cells_as_lines(json, outputs).into_iter().map(Arc::from).collect() };
        let before = compare_host::FileSide::Lines { name: crate::i18n::tf!("{name} at the last commit, cell by cell"), lines: lines(&String::from_utf8_lossy(&bytes)) };
        let after = compare_host::FileSide::Lines { name: crate::i18n::tf!("{name} now, cell by cell"), lines: lines(&now) };
        self.open_file_compare_sides(before, after);
    }
}

impl NotebookState {
    /// The keys: Shift+Enter and Ctrl+Enter run the cell in either mode; Esc leaves a cell's editor; in command
    /// mode (no editor has the keyboard) Jupyter's one-letter commands work.
    fn read_keys(&mut self, ctx: &eframe::egui::Context, acts: &mut Vec<Act>) {
        let focused = ctx.memory(|m| m.focused());
        let notebook_focused = focused.is_none() || focused == Some(views::notebook_id());
        let in_editor = self.selected_uid().is_some_and(|uid| focused == Some(editor_id(uid)));
        if !notebook_focused && !in_editor {
            return;
        }
        // The longer combinations first, as plain Enter also matches them.
        let mut found = None;
        ctx.input_mut(|i| {
            for (modifiers, act) in [(Modifiers::ALT, Act::RunCellInsert), (Modifiers::COMMAND, Act::RunCellStay), (Modifiers::SHIFT, Act::RunCell)] {
                if found.is_none() && i.consume_key(modifiers, Key::Enter) {
                    found = Some(act);
                }
            }
            if found.is_none() && i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Minus) {
                found = Some(Act::Split);
            }
        });
        if let Some(act) = found {
            acts.push(act);
            return;
        }
        if in_editor {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                ctx.memory_mut(|m| m.request_focus(views::notebook_id()));
                self.editing = false;
            }
            return;
        }
        let pressed: Vec<(Key, bool)> = ctx.input_mut(|i| {
            let mut keys = Vec::new();
            i.events.retain(|event| match event {
                eframe::egui::Event::Key { key, pressed: true, modifiers, repeat: false, .. } if !modifiers.command && !modifiers.alt && !modifiers.ctrl => {
                    keys.push((*key, modifiers.shift));
                    false
                }
                _ => true,
            });
            keys
        });
        for (key, shift) in pressed {
            if let Some(act) = self.command_key(key, shift) {
                acts.push(act);
            }
        }
    }
}

/// Whether a file's name is a notebook's.
pub(super) fn is_notebook_name(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ipynb"))
}
