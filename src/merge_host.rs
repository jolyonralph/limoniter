//! Merge, in a tab: the original, mine and theirs side by side above, read only and scrolling together, with what
//! changed coloured, and the merged result below to edit. Lines only one side changed, or both changed alike, are
//! already in the result; each conflict is there between markers until it's settled, with a button or by hand.
//! It opens for three files, for a file Git stopped a merge in (from Git's own copies of each side), for a file
//! with conflict markers in it, and from the command line for `git mergetool`, when the program's exit code says
//! whether the merge was finished. This part also sets Git up to use Limoniter for its diff and merge tools.

use super::*;
use crate::git_setup;
use crate::merge3::{self, Chunk, Kind, Labels, Marked, Take};
use eframe::egui::{Align2, Color32, FontId, Rect, Sense, pos2, vec2};
use std::sync::atomic::AtomicI32;

/// The exit code for a run from `--merge`: 0 once the result is saved with no conflicts left, 1 otherwise.
static EXIT_CODE: AtomicI32 = AtomicI32::new(1);
/// This run was started to do one merge, and ends when it's done.
static WAITING: AtomicBool = AtomicBool::new(false);

/// What the program's exit code should be, for a run started with `--merge`.
pub fn tool_exit_code() -> Option<i32> {
    WAITING.load(Ordering::SeqCst).then(|| EXIT_CODE.load(Ordering::SeqCst))
}

/// What the command line asked for, beyond opening files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolLaunch {
    /// `--diff LEFT RIGHT`
    Diff(PathBuf, PathBuf),
    /// `--merge BASE MINE THEIRS RESULT`
    Merge { base: PathBuf, mine: PathBuf, theirs: PathBuf, result: PathBuf },
}

/// What `--help` says.
pub const USAGE: &str = "Limoniter, a text and hex editor.

Usage:
  limoniter [FILE...]                         Opens the files.
  limoniter --diff LEFT RIGHT                 Compares two files side by side.
  limoniter --merge BASE MINE THEIRS RESULT   Merges MINE and THEIRS, both changed from BASE, into RESULT.
                                              The program waits until the merge tab is closed, then exits
                                              with 0 if RESULT was saved with no conflicts left, 1 if not,
                                              as git mergetool expects.
  limoniter --screenshot PICTURE [FILE...]    Saves a picture of the window, then closes.
  limoniter --help                            Shows this.

Git can use Limoniter for git difftool and git mergetool: Git, Use Limoniter for Git diff and merge.";

/// Works out what the command line asks for. Ok(None) for nothing special; Err for a mistake, to be told.
pub fn parse_tool_args(args: &[std::ffi::OsString]) -> Result<Option<ToolLaunch>, String> {
    let at = |flag: &str| args.iter().position(|arg| arg == flag);
    let after = |i: usize, count: usize, flag: &str, names: &str| -> Result<Vec<PathBuf>, String> {
        let got: Vec<PathBuf> = args.iter().skip(i + 1).take(count).map(PathBuf::from).collect();
        if got.len() < count { Err(format!("{flag} needs {names}.")) } else { Ok(got) }
    };
    if let Some(i) = at("--diff") {
        let files = after(i, 2, "--diff", "two files: LEFT RIGHT")?;
        return Ok(Some(ToolLaunch::Diff(files[0].clone(), files[1].clone())));
    }
    if let Some(i) = at("--merge") {
        let files = after(i, 4, "--merge", "four files: BASE MINE THEIRS RESULT")?;
        return Ok(Some(ToolLaunch::Merge { base: files[0].clone(), mine: files[1].clone(), theirs: files[2].clone(), result: files[3].clone() }));
    }
    Ok(None)
}

/// The merge for a tab: each side's lines, how they line up, and where the result is going.
pub(super) struct MergeTab {
    /// What's shown above each pane: the original, mine, theirs.
    names: [String; 3],
    lines: [Vec<Arc<str>>; 3],
    chunks: Vec<Chunk>,
    /// Each row across the three panes: the line shown in each (None where a side has fewer), and its chunk.
    rows: Vec<([Option<usize>; 3], usize)>,
    /// The first row of each chunk.
    chunk_rows: Vec<usize>,
    longest: usize,
    across: usize,
    scroll_to: Option<usize>,
    /// The conflicts marked in the result, and the version of it they were found in.
    marked: (u64, Vec<Marked>),
    /// Git stopped a merge in this file: the repository and the file in it, to mark it settled once it's saved.
    pub git: Option<(PathBuf, String)>,
    /// The result was last saved with no conflicts left in it.
    pub saved_clean: bool,
}

impl MergeTab {
    fn new(names: [String; 3], chunks: Vec<Chunk>) -> Self {
        let sides = merge3::sides(&chunks);
        let lines = sides.map(|side| side.into_iter().map(Arc::from).collect::<Vec<Arc<str>>>());
        let mut rows = Vec::new();
        let mut chunk_rows = Vec::new();
        let mut at = [0usize; 3];
        for (index, chunk) in chunks.iter().enumerate() {
            chunk_rows.push(rows.len());
            let lens = [chunk.base.len(), chunk.mine.len(), chunk.theirs.len()];
            for i in 0..lens.iter().copied().max().unwrap_or(0) {
                rows.push((std::array::from_fn(|side| (i < lens[side]).then_some(at[side] + i)), index));
            }
            for side in 0..3 {
                at[side] += lens[side];
            }
        }
        let longest = lines.iter().flatten().map(|line| line.chars().count()).max().unwrap_or(0);
        MergeTab { names, lines, chunks, rows, chunk_rows, longest, across: 0, scroll_to: None, marked: (u64::MAX, Vec::new()), git: None, saved_clean: false }
    }

    /// The conflict chunk a marked conflict in the result came from, going by what's on each side.
    fn chunk_of(&self, lines: &[Arc<str>], conflict: &Marked) -> Option<usize> {
        let clean = |take: Take| merge3::take(lines, conflict, take).into_iter().map(|line| line.strip_suffix('\r').map(str::to_string).unwrap_or(line)).collect::<Vec<_>>();
        let (mine, theirs) = (clean(Take::Mine), clean(Take::Theirs));
        self.chunks.iter().position(|chunk| chunk.kind == Kind::Conflict && chunk.mine == mine && chunk.theirs == theirs)
    }

    /// How many conflicts the merge had to begin with.
    fn conflicts_at_first(&self) -> usize {
        self.chunks.iter().filter(|chunk| chunk.kind == Kind::Conflict).count()
    }
}

/// The window's merge state beyond each tab's.
#[derive(Default)]
pub(super) struct MergeState {
    /// The tab whose closing ends this run, for `--merge`.
    pub waiting: Option<u64>,
    /// The exit code this run is to end with, as last worked out.
    pub exit_code: Option<i32>,
    /// For the file in front: its id and version, and how many conflicts are marked in it.
    marks: Option<(u64, u64, usize)>,
    /// Files the offer to resolve in the merge tab has been put away for.
    dismissed: std::collections::HashSet<u64>,
}

/// A file's text, in its own encoding, and that encoding. Text only.
fn text_of(bytes: &[u8], name: &str) -> Result<(String, TextFormat), String> {
    if codec::looks_binary(bytes) {
        return Err(format!("{name} looks like a binary file, so it can't be merged as lines of text."));
    }
    let utf8_bom = b"\xEF\xBB\xBF";
    let body = bytes.strip_prefix(utf8_bom).unwrap_or(bytes);
    if let Ok(text) = std::str::from_utf8(body) {
        let format = TextFormat { has_bom: body.len() != bytes.len(), ..TextFormat::default() };
        return Ok((text.to_string(), format));
    }
    let found = codec::detect(bytes);
    let mut format = TextFormat { charset: found.charset, ..TextFormat::default() };
    let decoded = codec::decode(bytes, &mut format);
    let eol = if decoded.counts.predominant(Eol::Lf) == Eol::CrLf { "\r\n" } else { "\n" };
    Ok((decoded.lines.iter().map(|line| format.readable_text(line)).collect::<Vec<_>>().join(eol), format))
}

/// A file's text for merging. A file that isn't there is empty, as Git's original is when both sides added it.
fn text_of_file(path: &Path) -> Result<(String, TextFormat), String> {
    match std::fs::read(path) {
        Ok(bytes) => text_of(&bytes, &path.display().to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok((String::new(), TextFormat::default())),
        Err(error) => Err(format!("{} couldn't be read: {error}", path.display())),
    }
}

/// The name a file goes by above its pane.
fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

thread_local! {
    /// The answers tests give to the questions the merge asks, in turn. Yes when there are none left.
    static TEST_ANSWERS: std::cell::RefCell<std::collections::VecDeque<bool>> = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}

fn ask(title: &str, question: &str) -> bool {
    if cfg!(test) {
        return TEST_ANSWERS.with(|answers| answers.borrow_mut().pop_front()).unwrap_or(true);
    }
    ask_yes_no(title, question)
}

/// Colours for each pane on a row of a chunk of this kind: the original, mine, theirs.
fn backs(kind: Kind) -> [Option<Color32>; 3] {
    match kind {
        Kind::Unchanged => [None, None, None],
        Kind::Mine => [None, Some(theme::compare_added()), None],
        Kind::Theirs => [None, None, Some(theme::compare_added())],
        Kind::Both => [None, Some(theme::compare_changed()), Some(theme::compare_changed())],
        Kind::Conflict => [Some(theme::problem_line()); 3],
    }
}

const ROW_TEXT: f32 = 13.0;

impl Limoniter {
    /// Opens a merge of three texts into a tab, its result to be saved to a file. Lines that merge cleanly are
    /// already in the result. Returns the tab's place, or None if it couldn't be opened (and why's been said).
    pub(super) fn open_merge(&mut self, sides: [(String, String); 3], format: TextFormat, result: &Path, git: Option<(PathBuf, String)>) -> Option<usize> {
        let full = std::path::absolute(result).unwrap_or_else(|_| result.to_path_buf());
        if let Some(open) = self.docs.iter().position(|doc| doc.path.as_deref().is_some_and(|p| same_path(p, &full))) {
            if self.docs[open].is_modified() {
                self.active = open;
                self.message("Merge", format!("{} is open with unsaved changes. Save or close it first, then merge again.", self.docs[open].name()));
                return None;
            }
            self.docs.remove(open);
            if self.active >= open && self.active > 0 {
                self.active -= 1;
            }
        }
        let [(base_name, base), (mine_name, mine), (theirs_name, theirs)] = sides;
        let merge = merge3::Merge::of(&base, &mine, &theirs);
        let text = merge.text(&Labels::default());
        let mut doc = Document::new(String::new());
        doc.path = Some(full.clone());
        doc.load_bytes(text.into_bytes(), &full, Some(TextFormat { charset: charset::known("utf-8"), has_bom: false, ..TextFormat::default() }), None);
        // Saved as mine was written.
        doc.text.format.charset = format.charset;
        doc.text.format.has_bom = format.has_bom;
        doc.format_chosen = true;
        doc.hex_mode = false;
        doc.disk_stamp = file_stamp(&full);
        doc.carried = true;
        let mut tab = MergeTab::new([base_name, mine_name, theirs_name], merge.chunks);
        tab.git = git;
        doc.merge = Some(Box::new(tab));
        if self.no_file() || (self.docs.len() == 1 && self.docs[0].is_blank()) {
            self.docs.clear();
        }
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
        self.pending_focus = true;
        self.merge_step(true);
        Some(self.active)
    }

    /// Merges three files on disk into a fourth.
    pub(super) fn open_merge_files(&mut self, base: &Path, mine: &Path, theirs: &Path, result: &Path) -> Option<usize> {
        let mut read = Vec::new();
        for path in [base, mine, theirs] {
            match text_of_file(path) {
                Ok(text) => read.push(text),
                Err(problem) => {
                    self.message("Merge", problem);
                    return None;
                }
            }
        }
        let format = read[1].1.clone();
        let texts: Vec<String> = read.into_iter().map(|(text, _)| text).collect();
        let [b, m, t]: [String; 3] = texts.try_into().ok()?;
        let names = [format!("Original: {}", name_of(base)), format!("Mine: {}", name_of(mine)), format!("Theirs: {}", name_of(theirs))];
        let [nb, nm, nt] = names;
        self.open_merge([(nb, b), (nm, m), (nt, t)], format, result, None)
    }

    /// Asks for the three files, and where the result goes.
    pub(super) fn merge_files_dialog(&mut self) {
        let pick = |title: &str| rfd::FileDialog::new().set_title(title).pick_file();
        let Some(base) = pick("Merge: the original both were changed from") else { return };
        let Some(mine) = pick("Merge: mine, one changed version") else { return };
        let Some(theirs) = pick("Merge: theirs, the other changed version") else { return };
        let mut dialog = rfd::FileDialog::new().set_title("Merge: save the result as").set_file_name(name_of(&mine));
        if let Some(folder) = mine.parent() {
            dialog = dialog.set_directory(folder);
        }
        let Some(result) = dialog.save_file() else { return };
        self.open_merge_files(&base, &mine, &theirs, &result);
    }

    /// A file Git stopped a merge in, opened for merging from Git's own copies: the common original (:1:), this
    /// branch's (:2:) and the one coming in (:3:).
    pub(super) fn resolve_git_conflict(&mut self, root: &Path, relative: &str) -> Option<usize> {
        let stage = |n: u8| crate::git::file_at(root, &format!(":{n}"), relative).unwrap_or_default();
        let mut sides = Vec::new();
        for (n, name) in [(1, "Original"), (2, "Mine"), (3, "Theirs")] {
            match text_of(&stage(n), relative) {
                Ok(text) => sides.push((format!("{name} (:{n}:{relative})"), text)),
                Err(problem) => {
                    self.message("Resolve in merge", problem);
                    return None;
                }
            }
        }
        let format = sides[1].1.1.clone();
        let mut sides = sides.into_iter().map(|(name, (text, _))| (name, text));
        let sides = [sides.next()?, sides.next()?, sides.next()?];
        self.open_merge(sides, format, &root.join(relative), Some((root.to_path_buf(), relative.to_string())))
    }

    /// The file in front, which has conflict markers in it, becomes a merge tab, the markers read as its conflicts.
    pub(super) fn resolve_marked(&mut self, index: usize) {
        let doc = &self.docs[index];
        let mut lines = doc.text.all_lines();
        // The empty line after a last line break isn't a line of either side.
        if lines.len() > 1 && lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        let Some(chunks) = merge3::parse_markers(&lines) else {
            return;
        };
        let names = ["Original (from the markers)".to_string(), "Mine".to_string(), "Theirs".to_string()];
        let git = doc.path.as_deref().and_then(|path| Some((crate::git::repository_of(path)?, path)))
            .and_then(|(root, path)| crate::git::relative(&root, path).map(|relative| (root, relative)));
        let mut tab = MergeTab::new(names, chunks);
        tab.git = git;
        self.docs[index].merge = Some(Box::new(tab));
        self.active = index;
        self.merge_step(true);
    }

    /// Resolves the file in front in the merge tab: from Git's copies if Git stopped a merge in it, or from its
    /// markers.
    pub(super) fn resolve_in_merge(&mut self) {
        let index = self.active;
        if self.docs[index].merge.is_some() {
            return;
        }
        if let Some((root, relative)) = self.git_file()
            && self.git.status.as_ref().and_then(|s| s.of(&relative)).is_some_and(|e| e.unstaged == Some(crate::git::Change::Conflicted))
        {
            if self.docs[index].is_modified() {
                if merge3::count_marked(&self.docs[index].text.all_lines()) > 0 {
                    self.resolve_marked(index);
                } else {
                    self.message("Resolve in merge", "Save this file first: the merge starts again from Git's copies of each side.".into());
                }
                return;
            }
            self.resolve_git_conflict(&root, &relative);
            return;
        }
        if merge3::count_marked(&self.docs[index].text.all_lines()) > 0 {
            self.resolve_marked(index);
        } else {
            self.message("Resolve in merge", "There are no conflict markers in this file.".into());
        }
    }

    /// The conflicts marked in the result, worked out again only when it's changed.
    fn merge_marks(&mut self, index: usize) -> Vec<Marked> {
        let doc = &mut self.docs[index];
        let version = doc.text.version();
        let Some(tab) = doc.merge.as_mut() else {
            return Vec::new();
        };
        if tab.marked.0 != version {
            tab.marked = (version, merge3::marked(&doc.text.all_lines()));
        }
        tab.marked.1.clone()
    }

    /// How many conflicts are still marked in a merge tab's result.
    pub(super) fn unresolved(&mut self, index: usize) -> usize {
        self.merge_marks(index).len()
    }

    /// Moves the result's cursor to the next or previous conflict, round to the first or last, and the panes above
    /// to where it came from.
    pub(super) fn merge_step(&mut self, forward: bool) {
        let index = self.active;
        let found = self.merge_marks(index);
        if found.is_empty() {
            return;
        }
        let caret = self.docs[index].text.caret_line;
        let target = if forward {
            found.iter().find(|c| c.start > caret).unwrap_or(&found[0])
        } else {
            found.iter().rev().find(|c| c.end < caret).unwrap_or(&found[found.len() - 1])
        };
        let target = *target;
        self.merge_go_to(index, &target);
    }

    fn merge_go_to(&mut self, index: usize, target: &Marked) {
        let lines = self.docs[index].text.all_lines();
        let doc = &mut self.docs[index];
        doc.text.move_caret(target.start, 0, false);
        doc.text.reveal = true;
        if let Some(tab) = doc.merge.as_mut()
            && let Some(chunk) = tab.chunk_of(&lines, target)
        {
            tab.scroll_to = Some(tab.chunk_rows[chunk]);
        }
        self.pending_focus = true;
    }

    /// Settles the conflict the cursor's in.
    pub(super) fn merge_take(&mut self, take: Take) {
        let index = self.active;
        let found = self.merge_marks(index);
        let caret = self.docs[index].text.caret_line;
        let Some(conflict) = found.iter().find(|c| c.start <= caret && caret <= c.end).copied() else {
            return;
        };
        let lines = self.docs[index].text.all_lines();
        let kept = merge3::take(&lines, &conflict, take);
        let text = &mut self.docs[index].text;
        text.replace_lines(conflict.start, conflict.end, kept);
        text.move_caret(conflict.start, 0, false);
        text.reveal = true;
        self.pending_focus = true;
    }

    /// Saves a merge tab's result, asking first if conflicts are left in it, and then offering to tell Git the
    /// file's settled.
    pub(super) fn save_merge(&mut self, index: usize) -> bool {
        let left = self.unresolved(index);
        if left > 0 {
            let question = format!("{} still has {left} conflict(s) not settled, with their markers in it. Save it anyway?", self.docs[index].name());
            if !ask("Save the merge", &question) {
                return false;
            }
        }
        let Some(path) = self.docs[index].path.clone() else {
            return false;
        };
        if !(self.write_to(index, &path) && self.wait_for_save(index)) {
            return false;
        }
        let clean = left == 0;
        let git = self.docs[index].merge.as_mut().and_then(|tab| {
            tab.saved_clean = clean;
            tab.git.clone()
        });
        if self.merges.waiting == Some(self.docs[index].id) {
            let code = if clean { 0 } else { 1 };
            self.merges.exit_code = Some(code);
            EXIT_CODE.store(code, Ordering::SeqCst);
        }
        if clean
            && let Some((root, relative)) = git
            && ask("Merge", &format!("{relative} is saved with every conflict settled. Mark it as resolved in Git (git add)?"))
        {
            if let Err(problem) = crate::git::stage(&root, &[relative]) {
                self.message("Merge", super::git_host::explain(&problem));
            }
            self.git.refresh();
        }
        true
    }

    /// A merge tab's been closed. For a run from `--merge`, that's the end of it.
    pub(super) fn merge_closed(&mut self, closed: &Document) {
        if self.merges.waiting != Some(closed.id) {
            return;
        }
        // Saved clean by Save, or written out by autosave with nothing left to settle and not changed since.
        let saved = !closed.is_modified() && merge3::count_marked(&closed.text.all_lines()) == 0;
        let clean = saved || closed.merge.as_ref().is_some_and(|tab| tab.saved_clean);
        let code = if clean { 0 } else { 1 };
        self.merges.exit_code = Some(code);
        EXIT_CODE.store(code, Ordering::SeqCst);
        if let Some(ctx) = &self.waker {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// Starts what the command line asked for.
    pub(super) fn start_tool(&mut self, launch: ToolLaunch) {
        match launch {
            ToolLaunch::Diff(left, right) => self.open_file_compare(left, right),
            ToolLaunch::Merge { base, mine, theirs, result } => {
                WAITING.store(true, Ordering::SeqCst);
                EXIT_CODE.store(1, Ordering::SeqCst);
                if let Some(index) = self.open_merge_files(&base, &mine, &theirs, &result) {
                    self.merges.waiting = Some(self.docs[index].id);
                }
            }
        }
    }

    /// Compares two files side by side.
    // Kept small on purpose: the side-by-side comparison it opens is to be replaced.
    pub(super) fn open_file_compare(&mut self, left: PathBuf, right: PathBuf) {
        match (Self::lines_of_file_on_disk(&left), Self::lines_of_file_on_disk(&right)) {
            (Ok((left_name, crate::big_jobs::Side::Lines(left))), Ok((right_name, crate::big_jobs::Side::Lines(right)))) => {
                let mut comparison = compare::Comparison::new(left_name, right_name, left, right);
                comparison.step(true);
                self.dialog = Dialog::Compare(Box::new(comparison));
            }
            (Err(problem), _) | (_, Err(problem)) => self.message("Compare", problem),
            _ => self.message("Compare", "One of these files is too large to compare from the command line.".into()),
        }
    }

    /// The bar offering to resolve a file with conflict markers in the merge tab.
    pub(super) fn marker_bar(&mut self, ui: &mut Ui) {
        let index = self.active;
        let doc = &self.docs[index];
        if doc.merge.is_some() || doc.hex_mode || doc.text.is_disk_backed() || doc.browser.is_some() || doc.text.line_count() > 200_000 || self.merges.dismissed.contains(&doc.id) {
            return;
        }
        let (id, version) = (doc.id, doc.text.version());
        let count = match self.merges.marks {
            Some((known, at, count)) if known == id && at == version => count,
            _ => {
                let count = merge3::count_marked(&doc.text.all_lines());
                self.merges.marks = Some((id, version, count));
                count
            }
        };
        if count == 0 {
            return;
        }
        let (mut resolve, mut dismiss) = (false, false);
        Panel::top("marker-bar").frame(egui::Frame::new().fill(theme::selected()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("This file has {count} conflict(s) marked in it.")).strong());
                resolve = ui.button("Resolve in merge\u{2026}").on_hover_text("Each side above, and the result to settle below").clicked();
                dismiss = ui.button("Not now").clicked();
            });
        });
        if resolve {
            self.resolve_marked(index);
        } else if dismiss {
            self.merges.dismissed.insert(id);
        }
    }

    /// The merge tab: the toolbar, the three sides, and the result.
    pub(super) fn merge_view(&mut self, ui: &mut Ui, view: ViewOptions, input_on: bool) -> Option<ContextCommand> {
        let index = self.active;
        let found = self.merge_marks(index);
        let caret = self.docs[index].text.caret_line;
        let here = found.iter().position(|c| c.start <= caret && caret <= c.end);
        let mut take = None;
        let (mut previous, mut next, mut save) = (false, false, false);
        let at_first = self.docs[index].merge.as_ref().map_or(0, |tab| tab.conflicts_at_first());
        egui::Frame::new().fill(theme::ribbon()).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let left = found.len();
                let said = match (left, at_first) {
                    (0, 0) => "No conflicts: everything merged by itself.".to_string(),
                    (0, _) => "Every conflict is settled.".to_string(),
                    (1, _) => "1 conflict left".to_string(),
                    (n, _) => format!("{n} conflicts left"),
                };
                let colour = if left > 0 { theme::rainbow_mistake() } else { theme::text() };
                ui.label(RichText::new(said).strong().color(colour));
                previous = ui.add_enabled(left > 0, Button::new("Previous conflict")).clicked();
                next = ui.add_enabled(left > 0, Button::new("Next conflict")).clicked();
                ui.separator();
                for (choice, label, tip) in [
                    (Take::Mine, "Take mine", "Just mine, above"),
                    (Take::Theirs, "Take theirs", "Just theirs, from the right"),
                    (Take::MineFirst, "Take both (mine first)", "Mine, then theirs"),
                    (Take::TheirsFirst, "Take both (theirs first)", "Theirs, then mine"),
                    (Take::Base, "Take the original", "The lines as they were before either side changed them"),
                ] {
                    if ui.add_enabled(here.is_some(), Button::new(label)).on_hover_text(tip).on_disabled_hover_text("Put the cursor in a conflict in the result first").clicked() {
                        take = Some(choice);
                    }
                }
                if here.is_none() && left > 0 {
                    ui.label(RichText::new("Or edit the result by hand.").color(theme::text_dim()));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    save = ui.button("Save the result").clicked();
                });
            });
        });
        let whole = ui.available_rect_before_wrap();
        let split = whole.min.y + (whole.height() * 0.48).round();
        let top = Rect::from_min_max(whole.min, pos2(whole.max.x, split));
        let label = Rect::from_min_max(pos2(whole.min.x, split), pos2(whole.max.x, split + 20.0));
        let bottom = Rect::from_min_max(pos2(whole.min.x, split + 20.0), whole.max);
        let mut clicked_chunk = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(top), |ui| {
            if let Some(tab) = self.docs[index].merge.as_mut() {
                clicked_chunk = sides_view(ui, tab);
            }
        });
        ui.painter().rect_filled(label, 0.0, theme::ribbon());
        let path = self.docs[index].path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        ui.painter().text(pos2(label.min.x + 10.0, label.center().y), Align2::LEFT_CENTER, format!("Result, to edit: {path}"), FontId::proportional(12.5), theme::text_dim());
        let doc = &mut self.docs[index];
        doc.update_syntax();
        let command = ui.scope_builder(egui::UiBuilder::new().max_rect(bottom), |ui| views::text_pane(ui, &mut doc.text, &mut doc.text_state, &mut self.clip, view, input_on, Pane::default())).inner;
        if let Some(choice) = take {
            self.merge_take(choice);
        } else if previous || next {
            self.merge_step(next);
        } else if save {
            self.save(index);
        } else if let Some(chunk) = clicked_chunk {
            // A conflict clicked above: the cursor goes to it in the result, if it's still there.
            let lines = self.docs[index].text.all_lines();
            let target = self.docs[index].merge.as_ref().and_then(|tab| found.iter().find(|c| tab.chunk_of(&lines, c) == Some(chunk)).copied());
            if let Some(target) = target {
                self.merge_go_to(index, &target);
            }
        }
        command
    }

    /// Makes Git use Limoniter for `git difftool` and `git mergetool`, asking first if it uses something else.
    pub(super) fn use_for_git(&mut self) {
        let global = git_setup::Global::default();
        let program = match std::env::current_exe() {
            Ok(program) => program,
            Err(problem) => return self.message("Git", format!("Couldn't tell where Limoniter is: {problem}")),
        };
        let Some(backup) = settings::data_folder().map(|folder| folder.join(git_setup::BACKUP_FILE)) else {
            return self.message("Git", "There's no settings folder to keep Git's settings in as they were.".into());
        };
        match git_setup::others(&global) {
            Ok(others) if !others.is_empty() => {
                let listed: Vec<String> = others.iter().map(|(key, value)| format!("{key} = {value}")).collect();
                let question = format!("Git is set to use another tool:\n\n{}\n\nUse Limoniter instead? What's set now is kept, and Stop using Limoniter for Git puts it back.", listed.join("\n"));
                if !ask("Use Limoniter for Git", &question) {
                    return;
                }
            }
            Ok(_) => {}
            Err(problem) => return self.message("Git", problem),
        }
        match git_setup::turn_on(&global, &program, &backup) {
            Ok(()) => self.message("Git", "Git now uses Limoniter for git difftool and git mergetool, for every repository on this computer.".into()),
            Err(problem) => self.message("Git", problem),
        }
    }

    /// Puts Git's diff and merge tool settings back as they were.
    pub(super) fn stop_using_for_git(&mut self) {
        let Some(backup) = settings::data_folder().map(|folder| folder.join(git_setup::BACKUP_FILE)) else {
            return;
        };
        let global = git_setup::Global::default();
        if !backup.exists() && !git_setup::is_on(&global) {
            return self.message("Git", "Git isn't set to use Limoniter, so there's nothing to put back.".into());
        }
        match git_setup::turn_off(&global, &backup) {
            Ok(()) => self.message("Git", "Git's diff and merge tool settings are back as they were before.".into()),
            Err(problem) => self.message("Git", problem),
        }
    }
}

/// The three sides, lined up and scrolling together. Returns the chunk of a row that was clicked.
fn sides_view(ui: &mut Ui, tab: &mut MergeTab) -> Option<usize> {
    const GAP: f32 = 6.0;
    let width = ui.available_width();
    let pane = ((width - 2.0 * GAP) / 3.0).floor();
    let (names, _) = ui.allocate_exact_size(vec2(width, 20.0), Sense::hover());
    for (side, name) in tab.names.iter().enumerate() {
        let x = names.min.x + side as f32 * (pane + GAP);
        let area = Rect::from_min_max(pos2(x, names.min.y), pos2(x + pane, names.max.y));
        ui.painter_at(area).text(pos2(area.min.x + 4.0, area.center().y), Align2::LEFT_CENTER, name, FontId::proportional(12.5), theme::text_dim());
    }
    let font = FontId::monospace(ROW_TEXT);
    let (cell, row_height) = ui.ctx().fonts_mut(|fonts| (fonts.glyph_width(&font, 'M'), fonts.row_height(&font).ceil() + 2.0));
    let digits = tab.lines.iter().map(Vec::len).max().unwrap_or(0).to_string().len().max(3);
    let number_width = (digits + 1) as f32 * cell;
    if tab.longest * (cell as usize).max(1) > pane as usize {
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut tab.across, 0..=tab.longest).show_value(false).text("Across"));
        });
    }
    let mut area = egui::ScrollArea::vertical().id_salt("merge-sides").auto_shrink([false, false]);
    if let Some(row) = tab.scroll_to.take() {
        area = area.vertical_scroll_offset(row.saturating_sub(3) as f32 * row_height);
    }
    let mut clicked = None;
    ui.spacing_mut().item_spacing.y = 0.0;
    let across = tab.across;
    area.show_rows(ui, row_height, tab.rows.len(), |ui, range| {
        for row in range {
            let (at, chunk) = tab.rows[row];
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), row_height), Sense::click());
            if response.clicked() {
                clicked = Some(chunk);
            }
            let colours = backs(tab.chunks[chunk].kind);
            for side in 0..3 {
                let x = rect.min.x + side as f32 * (pane + GAP);
                let area = Rect::from_min_max(pos2(x, rect.min.y), pos2(x + pane, rect.max.y));
                let painter = ui.painter_at(area);
                let Some(line) = at[side] else {
                    painter.rect_filled(area, 0.0, theme::ruler_back());
                    continue;
                };
                if let Some(colour) = colours[side] {
                    painter.rect_filled(area, 0.0, colour);
                }
                painter.text(pos2(area.min.x + number_width - cell, area.min.y + 1.0), Align2::RIGHT_TOP, line + 1, font.clone(), theme::gutter_text());
                let fits = ((area.width() - number_width) / cell).ceil() as usize + 1;
                let shown: String = text_buffer::display_text(&tab.lines[side][line]).chars().skip(across).take(fits).collect();
                painter.text(pos2(area.min.x + number_width, area.min.y + 1.0), Align2::LEFT_TOP, shown, font.clone(), theme::editor_text());
            }
        }
    });
    clicked
}

#[cfg(test)]
mod tests {
    use super::super::tests::{app_with, draw};
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let folder = std::env::temp_dir().join(format!("limoniter-merge-{tag}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn answer(answers: &[bool]) {
        TEST_ANSWERS.with(|queue| *queue.borrow_mut() = answers.iter().copied().collect());
    }

    fn three(folder: &Path, base: &str, mine: &str, theirs: &str) -> [PathBuf; 4] {
        let paths = ["base.txt", "mine.txt", "theirs.txt", "result.txt"].map(|name| folder.join(name));
        for (path, text) in paths.iter().zip([base, mine, theirs]) {
            std::fs::write(path, text).unwrap();
        }
        paths
    }

    #[test]
    fn the_command_line_is_read() {
        let args = |list: &[&str]| list.iter().map(std::ffi::OsString::from).collect::<Vec<_>>();
        assert_eq!(parse_tool_args(&args(&["a.txt"])).unwrap(), None);
        assert_eq!(parse_tool_args(&args(&["--diff", "l", "r"])).unwrap(), Some(ToolLaunch::Diff("l".into(), "r".into())));
        assert_eq!(
            parse_tool_args(&args(&["--merge", "b", "m", "t", "out"])).unwrap(),
            Some(ToolLaunch::Merge { base: "b".into(), mine: "m".into(), theirs: "t".into(), result: "out".into() })
        );
        assert!(parse_tool_args(&args(&["--merge", "b", "m", "t"])).is_err());
        assert!(parse_tool_args(&args(&["--diff", "l"])).is_err());
        assert!(USAGE.contains("--diff LEFT RIGHT") && USAGE.contains("--merge BASE MINE THEIRS RESULT"));
    }

    #[test]
    fn a_merge_opens_with_clean_changes_taken_and_conflicts_marked() {
        let folder = scratch("open");
        let [base, mine, theirs, result] = three(&folder, "a\nb\nc\nd\ne\n", "A\nb\nc\nmine\ne\n", "a\nb\nC\ntheirs\ne\n");
        let (mut app, ctx) = app_with("");
        let index = app.open_merge_files(&base, &mine, &theirs, &result).unwrap();
        assert!(app.docs[index].merge.is_some());
        assert!(app.docs[index].is_modified(), "the result isn't saved until it's asked for");
        assert_eq!(app.unresolved(index), 1);
        // A was only mine's; c and d were changed by both, differently, into one conflict.
        let lines: Vec<String> = app.docs[index].text.all_lines().iter().map(|l| l.to_string()).collect();
        assert_eq!(lines[0], "A");
        assert!(lines.contains(&"<<<<<<< mine".to_string()));
        assert_eq!(app.docs[index].text.caret_line, 2, "the cursor starts at the first conflict");
        for _ in 0..3 {
            draw(&mut app, &ctx, Vec::new());
        }
        app.merge_take(Take::TheirsFirst);
        assert_eq!(app.unresolved(index), 0);
        let text: Vec<String> = app.docs[index].text.all_lines().iter().map(|l| l.to_string()).collect();
        assert_eq!(text, ["A", "b", "C", "theirs", "c", "mine", "e", ""]);
        assert_eq!(app.docs[index].merge.as_ref().unwrap().conflicts_at_first(), 1);
        draw(&mut app, &ctx, Vec::new());
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn each_way_of_settling_a_conflict() {
        let folder = scratch("takes");
        let [base, mine, theirs, result] = three(&folder, "x\nold\ny\n", "x\nM\ny\n", "x\nT\ny\n");
        for (take, expected) in [(Take::Mine, vec!["M"]), (Take::Theirs, vec!["T"]), (Take::MineFirst, vec!["M", "T"]), (Take::TheirsFirst, vec!["T", "M"])] {
            let (mut app, _ctx) = app_with("");
            let index = app.open_merge_files(&base, &mine, &theirs, &result).unwrap();
            app.merge_take(take);
            let lines: Vec<String> = app.docs[index].text.all_lines().iter().map(|l| l.to_string()).collect();
            let mut want = vec!["x"];
            want.extend(expected);
            want.extend(["y", ""]);
            assert_eq!(lines, want, "{take:?}");
        }
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn next_and_previous_go_round_the_conflicts() {
        let folder = scratch("step");
        let [base, mine, theirs, result] = three(&folder, "1\n2\n3\n4\n5\n", "1\nM2\n3\n4\nM5\n", "1\nT2\n3\n4\nT5\n");
        let (mut app, _ctx) = app_with("");
        let index = app.open_merge_files(&base, &mine, &theirs, &result).unwrap();
        let found = app.merge_marks(index);
        assert_eq!(found.len(), 2);
        assert_eq!(app.docs[index].text.caret_line, found[0].start);
        app.merge_step(true);
        assert_eq!(app.docs[index].text.caret_line, found[1].start);
        let row = app.docs[index].merge.as_ref().unwrap().scroll_to;
        assert_eq!(row, Some(app.docs[index].merge.as_ref().unwrap().chunk_rows[3]), "the panes above go to the same conflict");
        app.merge_step(true);
        assert_eq!(app.docs[index].text.caret_line, found[0].start, "round to the first");
        app.merge_step(false);
        assert_eq!(app.docs[index].text.caret_line, found[1].start, "and back round to the last");
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn the_panes_line_up_every_side() {
        let chunks = merge3::merge(&["a".into(), "b".into()], &["a".into(), "b1".into(), "b2".into()], &["a".into()]);
        let tab = MergeTab::new(Default::default(), chunks);
        assert_eq!(tab.rows.len(), 3);
        assert_eq!(tab.rows[0].0, [Some(0), Some(0), Some(0)]);
        assert_eq!(tab.rows[1].0, [Some(1), Some(1), None]);
        assert_eq!(tab.rows[2].0, [None, Some(2), None]);
    }

    /// What `--merge` from git mergetool ends with: 0 only for a result saved with no conflicts left.
    #[test]
    fn the_exit_code_says_whether_the_merge_was_finished() {
        let folder = scratch("exit");
        let [base, mine, theirs, result] = three(&folder, "a\nb\n", "a\nmine\n", "a\ntheirs\n");
        let launch = ToolLaunch::Merge { base: base.clone(), mine: mine.clone(), theirs: theirs.clone(), result: result.clone() };

        // Closed without saving: 1.
        let (mut app, _ctx) = app_with("");
        app.start_tool(launch.clone());
        let index = app.active;
        assert_eq!(app.merges.waiting, Some(app.docs[index].id));
        let doc = app.docs.remove(index);
        app.merge_closed(&doc);
        assert_eq!(app.merges.exit_code, Some(1));

        // Saved with the conflict still there (after saying yes to that): 1, and the markers are in the file.
        let (mut app, _ctx) = app_with("");
        app.start_tool(launch.clone());
        let index = app.active;
        answer(&[false]);
        assert!(!app.save(index), "saying no to saving with conflicts left saves nothing");
        assert!(!result.exists());
        answer(&[true]);
        assert!(app.save(index));
        assert!(std::fs::read_to_string(&result).unwrap().contains("<<<<<<< mine"));
        let doc = app.docs.remove(index);
        app.merge_closed(&doc);
        assert_eq!(app.merges.exit_code, Some(1));

        // Settled and saved: 0.
        let (mut app, _ctx) = app_with("");
        app.start_tool(launch);
        let index = app.active;
        app.merge_take(Take::Mine);
        assert!(app.save(index));
        assert_eq!(std::fs::read_to_string(&result).unwrap(), "a\nmine\n");
        assert_eq!(app.merges.exit_code, Some(0));
        assert!(!app.docs[index].is_modified());
        let doc = app.docs.remove(index);
        app.merge_closed(&doc);
        assert_eq!(app.merges.exit_code, Some(0));
        assert_eq!(tool_exit_code(), Some(EXIT_CODE.load(Ordering::SeqCst)));
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn closing_the_tab_runs_the_exit_through_close_tab() {
        let folder = scratch("close");
        let [base, mine, theirs, result] = three(&folder, "a\n", "a\nb\n", "a\n");
        let (mut app, _ctx) = app_with("");
        app.start_tool(ToolLaunch::Merge { base, mine, theirs, result: result.clone() });
        let index = app.active;
        assert_eq!(app.unresolved(index), 0);
        assert!(app.save(index));
        app.close_tab(index);
        assert_eq!(app.merges.exit_code, Some(0));
        assert_eq!(std::fs::read_to_string(&result).unwrap(), "a\nb\n");
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn crlf_and_another_encoding_are_kept_in_the_result() {
        let folder = scratch("format");
        let paths = ["base.txt", "mine.txt", "theirs.txt", "result.txt"].map(|name| folder.join(name));
        std::fs::write(&paths[0], b"caf\xe9\r\nmiddle\r\none\r\n").unwrap();
        std::fs::write(&paths[1], b"caf\xe9\r\nmiddle\r\nONE\r\n").unwrap();
        std::fs::write(&paths[2], b"caf\xe9!\r\nmiddle\r\none\r\n").unwrap();
        let (mut app, _ctx) = app_with("");
        let index = app.open_merge_files(&paths[0], &paths[1], &paths[2], &paths[3]).unwrap();
        assert_eq!(app.unresolved(index), 0);
        assert!(app.save(index));
        assert_eq!(std::fs::read(&paths[3]).unwrap(), b"caf\xe9!\r\nmiddle\r\nONE\r\n");
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_file_with_markers_becomes_a_merge_tab_in_place() {
        let folder = scratch("markers");
        let path = folder.join("marked.txt");
        std::fs::write(&path, "top\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> other\nend\n").unwrap();
        let (mut app, ctx) = app_with("");
        assert!(app.open_path(&path));
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(app.merges.marks.map(|(_, _, count)| count), Some(1), "the bar has counted the conflict");
        let index = app.active;
        app.resolve_in_merge();
        assert!(app.docs[index].merge.is_some());
        let tab = app.docs[index].merge.as_ref().unwrap();
        assert_eq!(tab.lines[1].iter().map(|l| l.to_string()).collect::<Vec<_>>(), ["top", "ours", "end"]);
        assert_eq!(tab.lines[2].iter().map(|l| l.to_string()).collect::<Vec<_>>(), ["top", "theirs", "end"]);
        draw(&mut app, &ctx, Vec::new());
        app.merge_take(Take::Theirs);
        assert!(app.save(index));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "top\ntheirs\nend\n");
        std::fs::remove_dir_all(folder).unwrap();
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new(crate::git::program().unwrap())
            .current_dir(root)
            .args(["-c", "user.name=T", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", root.join("no-such-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned() + &*String::from_utf8_lossy(&output.stderr)
    }

    /// A repository with a merge stopped on a conflict in notes.txt.
    fn conflicted_repo(tag: &str) -> PathBuf {
        let root = scratch(tag);
        git(&root, &["init", "-q"]);
        std::fs::write(root.join("notes.txt"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
        git(&root, &["add", "notes.txt"]);
        git(&root, &["commit", "-q", "-m", "base"]);
        git(&root, &["checkout", "-q", "-b", "other"]);
        std::fs::write(root.join("notes.txt"), "one\nTWO (theirs)\nthree\nfour\nfive\n").unwrap();
        git(&root, &["commit", "-q", "-am", "theirs"]);
        git(&root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("notes.txt"), "one\ntwo (mine)\nthree\nfour\nFIVE\n").unwrap();
        git(&root, &["commit", "-q", "-am", "mine"]);
        git(&root, &["merge", "-q", "other"]);
        root
    }

    #[test]
    fn a_file_git_stopped_a_merge_in_is_merged_from_git_s_copies_and_marked_resolved() {
        if crate::git::program().is_none() {
            return;
        }
        let root = conflicted_repo("git");
        assert!(git(&root, &["status", "--porcelain"]).contains("UU notes.txt"));
        let (mut app, _ctx) = app_with("");
        let index = app.resolve_git_conflict(&root, "notes.txt").unwrap();
        let tab = app.docs[index].merge.as_ref().unwrap();
        assert_eq!(tab.lines[0].iter().map(|l| l.to_string()).collect::<Vec<_>>(), ["one", "two", "three", "four", "five"]);
        assert_eq!(tab.git.as_ref().map(|(_, relative)| relative.as_str()), Some("notes.txt"));
        assert_eq!(app.unresolved(index), 1);
        app.merge_take(Take::MineFirst);
        answer(&[true]);
        assert!(app.save(index));
        assert_eq!(std::fs::read_to_string(root.join("notes.txt")).unwrap(), "one\ntwo (mine)\nTWO (theirs)\nthree\nfour\nFIVE\n");
        assert!(git(&root, &["status", "--porcelain"]).contains("M  notes.txt"), "git add was run: {}", git(&root, &["status", "--porcelain"]));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Git's mergetool and difftool, set up as Use Limoniter for Git sets them, run a stand-in for the program
    /// that writes down what it was given, and git takes its exit code.
    #[cfg(unix)]
    #[test]
    fn git_runs_the_command_lines_it_was_given() {
        if crate::git::program().is_none() {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let root = conflicted_repo("tools");
        let config = root.join("global-config");
        let global = git_setup::Global { file: Some(config.clone()) };
        let stub = root.join("stub dir").join("limoniter $stub");
        std::fs::create_dir_all(stub.parent().unwrap()).unwrap();
        let log = root.join("calls.txt");
        std::fs::write(&stub, format!("#!/bin/sh\nfor a in \"$@\"; do printf '%s|' \"$a\" >> '{}'; done\necho >> '{}'\nif [ \"$1\" = --merge ]; then printf 'merged\\n' > \"$5\"; fi\nexit 0\n", log.display(), log.display())).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        git_setup::turn_on(&global, &stub, &root.join("before.json")).unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new(crate::git::program().unwrap())
                .current_dir(&root)
                .args(args)
                .env("GIT_CONFIG_GLOBAL", &config)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .stdin(std::process::Stdio::null())
                .output()
                .unwrap()
        };
        let merged = run(&["mergetool", "--no-prompt", "notes.txt"]);
        assert!(merged.status.success(), "{}", String::from_utf8_lossy(&merged.stderr));
        let calls = std::fs::read_to_string(&log).unwrap();
        let first: Vec<&str> = calls.lines().next().unwrap().trim_end_matches('|').split('|').collect();
        assert_eq!(first[0], "--merge");
        assert_eq!(first.len(), 5, "{first:?}");
        assert!(first[1].contains("BASE") && first[2].contains("LOCAL") && first[3].contains("REMOTE"), "{first:?}");
        assert_eq!(first[4], "notes.txt");
        assert_eq!(std::fs::read_to_string(root.join("notes.txt")).unwrap(), "merged\n");
        assert!(String::from_utf8_lossy(&run(&["status", "--porcelain"]).stdout).contains("M  notes.txt"), "an exit code of 0 is trusted, and git marks it resolved");
        run(&["commit", "-q", "-m", "merged", "-c", "user.name=T"]);
        // Difftool, for a change not yet committed.
        std::fs::write(root.join("notes.txt"), "changed\n").unwrap();
        let diffed = run(&["difftool", "--no-prompt", "notes.txt"]);
        assert!(diffed.status.success(), "{}", String::from_utf8_lossy(&diffed.stderr));
        let calls = std::fs::read_to_string(&log).unwrap();
        let last: Vec<&str> = calls.lines().last().unwrap().trim_end_matches('|').split('|').collect();
        assert_eq!(last[0], "--diff");
        assert_eq!(last.len(), 3, "{last:?}");
        assert_eq!(last[2], "notes.txt");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn git_takes_a_failing_exit_code_as_not_merged() {
        if crate::git::program().is_none() {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let root = conflicted_repo("tools-fail");
        let config = root.join("global-config");
        let global = git_setup::Global { file: Some(config.clone()) };
        let stub = root.join("limoniter");
        std::fs::write(&stub, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        git_setup::turn_on(&global, &stub, &root.join("before.json")).unwrap();
        let output = std::process::Command::new(crate::git::program().unwrap())
            .current_dir(&root)
            .args(["mergetool", "--no-prompt", "notes.txt"])
            .env("GIT_CONFIG_GLOBAL", &config)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(!output.status.success());
        let status = std::process::Command::new(crate::git::program().unwrap()).current_dir(&root).args(["status", "--porcelain"]).env("GIT_CONFIG_GLOBAL", &config).output().unwrap();
        assert!(String::from_utf8_lossy(&status.stdout).contains("UU notes.txt"), "still in conflict");
        std::fs::remove_dir_all(root).unwrap();
    }
}
