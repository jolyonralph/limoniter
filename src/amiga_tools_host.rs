//! The window's side of the Amiga demo tools (Run on, Amiga crunchers and music): crunching the program in front,
//! or the one chosen in a disk image's browser, with Shrinkler or Cranker; converting the module in front for
//! LightSpeedPlayer or The Player 6.1, and putting the replay calls into 68000 source; and, when the setting is on,
//! crunching what F9 builds. Every tool runs in the background, and a missing tool only ever means a note.

use super::{Dialog, Limoniter, theme};
use crate::amiga_tools::{self as tools, Outcome, Prefs as ToolPrefs, Replay, Tool};
use crate::assemblers::Cpu;
use eframe::egui::{self, Button, Context, Key, RichText};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Crunch,
    Music,
    Tools,
}

/// The file the box works on: on the disk, or taken out of an image or not saved yet.
struct Source {
    name: String,
    path: Option<PathBuf>,
    bytes: Option<Arc<Vec<u8>>>,
    /// Where what's made goes.
    folder: Option<PathBuf>,
    program: bool,
    module: Option<crate::protracker::Module>,
}

/// A tool running in the background.
pub(super) struct Job {
    what: String,
    stop: Arc<AtomicBool>,
    rx: Receiver<Result<Outcome, String>>,
}

pub(super) struct AmigaToolsBox {
    page: Page,
    prefs: ToolPrefs,
    found: Vec<(Tool, Option<PathBuf>)>,
    source: Option<Source>,
    cruncher: Tool,
    output_name: String,
    replay: Replay,
    job: Option<Job>,
    result: Option<Result<Outcome, String>>,
    downloading: Option<(Tool, Receiver<Result<PathBuf, String>>)>,
    status: String,
}

fn find_all(prefs: &ToolPrefs) -> Vec<(Tool, Option<PathBuf>)> {
    Tool::ALL.into_iter().map(|t| (t, prefs.find(t))).collect()
}

fn found(found: &[(Tool, Option<PathBuf>)], tool: Tool) -> Option<PathBuf> {
    found.iter().find(|(t, _)| *t == tool).and_then(|(_, p)| p.clone())
}

/// Starts work on a thread of its own, which asks for the window to be redrawn when it's done.
fn start(ctx: Option<&Context>, what: String, work: impl FnOnce(&AtomicBool) -> Result<Outcome, String> + Send + 'static) -> Job {
    let (tx, rx) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let (flag, ctx) = (stop.clone(), ctx.cloned());
    std::thread::spawn(move || {
        let _ = tx.send(work(&flag));
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    });
    Job { what, stop, rx }
}

/// The file in a scratch folder of its own, for a tool to read when it isn't on the disk as it is.
fn scratch_copy(name: &str, bytes: &[u8]) -> Result<(PathBuf, PathBuf), String> {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let folder = std::env::temp_dir().join(format!("limoniter-amiga-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).map_err(|e| crate::i18n::tf!("A folder to work in couldn't be made: {e}"))?;
    // Only the file's own name, whatever path it came with.
    let safe = name.rsplit(['/', '\\', ':', '\u{203A}']).next().unwrap_or(name).trim();
    let path = folder.join(if safe.is_empty() { "file" } else { safe });
    std::fs::write(&path, bytes).map_err(|e| crate::i18n::tf!("{} couldn't be written: {e}", path.display()))?;
    Ok((folder, path))
}

/// The lines for the Output panel about what a tool did.
fn report(what: &str, result: &Result<Outcome, String>) -> Vec<String> {
    let mut lines = vec![what.to_string()];
    match result {
        Ok(outcome) => {
            lines.extend(outcome.printed.iter().cloned());
            if outcome.ok {
                let sizes = outcome.sizes();
                if !sizes.is_empty() {
                    lines.push(format!("Crunched in {:.1} s: {sizes}.", outcome.seconds));
                }
                lines.extend(outcome.made.iter().map(|(path, size)| format!("Wrote {} ({} bytes).", path.display(), tools::group(*size))));
            } else {
                lines.push("It didn't finish: see what it printed above.".into());
            }
        }
        Err(problem) => lines.push(problem.clone()),
    }
    lines
}

impl Limoniter {
    /// The file the box is for: the file chosen in a browser tab, or the document in front.
    fn amiga_tools_source(&self) -> Option<Source> {
        if self.no_file() {
            return None;
        }
        let doc = &self.docs[self.active];
        let (name, path, bytes, folder) = if let Some(browser) = &doc.browser {
            let (name, bytes) = browser.selected_file()?.ok()?;
            (name, None, Some(Arc::new(bytes)), doc.path.as_deref().and_then(Path::parent).map(Path::to_path_buf))
        } else if let Some(path) = doc.path.clone().filter(|_| !doc.is_modified() && !doc.text.is_disk_backed()) {
            let name = doc.name();
            let folder = path.parent().map(Path::to_path_buf);
            (name, Some(path), None, folder)
        } else {
            let bytes = if doc.hex_mode { doc.hex.bytes() } else { doc.text.encode(&doc.text.format, crate::codec::Lossy::Omit).ok()? };
            let folder = doc.path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
            (doc.name(), None, Some(Arc::new(bytes)), folder)
        };
        let head = match (&bytes, &path) {
            (Some(bytes), _) => bytes.to_vec(),
            (None, Some(path)) => std::fs::read(path).ok()?,
            (None, None) => return None,
        };
        let program = tools::is_hunk_executable(&head);
        let module = crate::protracker::read(&head, &name).ok();
        Some(Source { name, path, bytes, folder, program, module })
    }

    /// Opens the box (Run on, Amiga crunchers and music), on the page that suits the file in front.
    pub(super) fn open_amiga_tools(&mut self) {
        let prefs = ToolPrefs::load();
        let source = self.amiga_tools_source();
        let page = match &source {
            Some(s) if s.module.is_some() => Page::Music,
            Some(_) => Page::Crunch,
            None => Page::Tools,
        };
        let output_name = source.as_ref().map(|s| tools::crunched_name(&s.name)).unwrap_or_default();
        let cruncher = prefs.after_build.unwrap_or(Tool::Shrinkler);
        let found = find_all(&prefs);
        let replay = if prefs.lsp.micro { Replay::LspMicro } else if prefs.lsp.insane { Replay::LspInsane } else { Replay::LspCia };
        self.dialog = Dialog::AmigaTools(Box::new(AmigaToolsBox { page, prefs, found, source, cruncher, output_name, replay, job: None, result: None, downloading: None, status: String::new() }));
    }

    /// The 68000 source a snippet goes into: the document in front if it's 68000 source, else the first open one.
    fn m68k_target(&self) -> Option<usize> {
        if self.doc_cpu(self.active) == Some(Cpu::M68k) {
            return Some(self.active);
        }
        (0..self.docs.len()).find(|&i| self.doc_cpu(i) == Some(Cpu::M68k) && !self.docs[i].text.read_only)
    }

    pub(super) fn amiga_tools_dialog(&mut self, ctx: &Context) -> bool {
        let target = self.m68k_target().map(|i| self.docs[i].name());
        let Dialog::AmigaTools(state) = &mut self.dialog else {
            return false;
        };
        let mut printed: Vec<String> = Vec::new();
        if let Some(job) = &state.job {
            match job.rx.try_recv() {
                Ok(result) => {
                    printed = report(&job.what, &result);
                    state.result = Some(result);
                    state.job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => state.job = None,
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(200)),
            }
        }
        if let Some((tool, rx)) = &state.downloading {
            match rx.try_recv() {
                Ok(Ok(path)) => {
                    state.status = format!("{} downloaded to {}.", tool.name(), path.display());
                    state.found = find_all(&state.prefs);
                    state.downloading = None;
                }
                Ok(Err(problem)) => {
                    state.status = problem;
                    state.downloading = None;
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(200)),
            }
        }
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut action: Option<Act> = None;
        let busy = state.job.is_some();
        egui::Window::new(crate::i18n::tr("Amiga crunchers and music")).collapsible(false).resizable(true).default_width(600.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(640.0);
            ui.horizontal(|ui| {
                for (page, label) in [(Page::Crunch, "Crunch a program"), (Page::Music, "Music for a replayer"), (Page::Tools, "Tools")] {
                    if ui.selectable_label(state.page == page, crate::i18n::tr(label)).clicked() {
                        state.page = page;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| match state.page {
                Page::Crunch => crunch_page(ui, state, &mut action),
                Page::Music => music_page(ui, state, target.as_deref(), &mut action),
                Page::Tools => tools_page(ui, state, &mut action),
            });
            ui.separator();
            if let Some(job) = &state.job {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(crate::i18n::tf!("{}\u{2026}", job.what));
                    if ui.button(crate::i18n::tr("Stop")).clicked() {
                        job.stop.store(true, Ordering::Relaxed);
                    }
                });
            } else if let Some(result) = &state.result {
                match result {
                    Ok(outcome) if outcome.ok => {
                        let sizes = outcome.sizes();
                        let made: Vec<String> = outcome.made.iter().filter_map(|(p, _)| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
                        let words = if sizes.is_empty() { format!("Made {}.", made.join(", ")) } else { format!("Done: {sizes}.") };
                        ui.colored_label(theme::change_saved(), words);
                    }
                    Ok(_) => {
                        ui.colored_label(theme::rainbow_mistake(), crate::i18n::tr("It didn't succeed: what it printed is in the Output panel."));
                    }
                    Err(problem) => {
                        ui.colored_label(theme::rainbow_mistake(), problem);
                    }
                }
            }
            if !state.status.is_empty() {
                ui.label(RichText::new(&state.status).color(theme::text_dim()));
            }
            ui.horizontal(|ui| {
                close |= ui.add_enabled(!busy, Button::new(crate::i18n::tr("Close"))).clicked();
            });
        });
        if let Some(act) = action {
            self.amiga_tools_act(ctx, act);
        }
        if let Dialog::AmigaTools(state) = &self.dialog
            && state.prefs != ToolPrefs::load()
        {
            state.prefs.save();
        }
        for (n, line) in printed.into_iter().enumerate() {
            self.script_print(line, n == 0);
        }
        if close && !busy {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }

    /// Does what was asked in the box.
    fn amiga_tools_act(&mut self, ctx: &Context, act: Act) {
        let target = self.m68k_target().map(|i| (i, self.docs[i].name()));
        match act {
            Act::Crunch => self.amiga_crunch(ctx),
            Act::Convert => self.amiga_convert(ctx),
            Act::Insert(text) => match target {
                Some((index, name)) => {
                    let doc = &mut self.docs[index];
                    doc.text.paste_text(&text, false);
                    self.active = index;
                    if let Dialog::AmigaTools(state) = &mut self.dialog {
                        state.status = format!("The replay calls were put into {name}, at the cursor.");
                    }
                }
                None => ctx.copy_text(text),
            },
            Act::Copy(text) => ctx.copy_text(text),
            Act::Download(tool) => {
                if let Dialog::AmigaTools(state) = &mut self.dialog {
                    let (tx, rx) = channel();
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(tools::download(tool));
                        ctx.request_repaint();
                    });
                    state.status = format!("Downloading {}\u{2026}", tool.name());
                    state.downloading = Some((tool, rx));
                }
            }
            Act::Choose(tool) => {
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tf!("Where is {}?", tool.name())).pick_file()
                    && let Dialog::AmigaTools(state) = &mut self.dialog
                {
                    state.prefs.set_path(tool, Some(path));
                    state.found = find_all(&state.prefs);
                }
            }
            Act::Forget(tool) => {
                if let Dialog::AmigaTools(state) = &mut self.dialog {
                    state.prefs.set_path(tool, None);
                    state.found = find_all(&state.prefs);
                }
            }
            Act::Folder => {
                if let Some(folder) = rfd::FileDialog::new().set_title(crate::i18n::tr("Where to put what's made")).pick_folder()
                    && let Dialog::AmigaTools(state) = &mut self.dialog
                    && let Some(source) = &mut state.source
                {
                    source.folder = Some(folder);
                }
            }
        }
    }

    fn amiga_crunch(&mut self, ctx: &Context) {
        let Dialog::AmigaTools(state) = &mut self.dialog else {
            return;
        };
        let Some(source) = &state.source else {
            return;
        };
        let tool = state.cruncher;
        let Some(program) = found(&state.found, tool) else {
            state.result = Some(Err(crate::i18n::tf!("{} wasn't found. {}", tool.name(), tool.how_to_get())));
            return;
        };
        let Some(folder) = source.folder.clone() else {
            state.result = Some(Err(crate::i18n::tr("Choose a folder for the crunched file first.").into()));
            return;
        };
        let output = folder.join(state.output_name.trim());
        if state.output_name.trim().is_empty() || source.path.as_deref() == Some(output.as_path()) {
            state.result = Some(Err(crate::i18n::tr("Give the crunched file a name of its own.").into()));
            return;
        }
        let prefs = state.prefs.clone();
        let (path, bytes, name) = (source.path.clone(), source.bytes.clone(), source.name.clone());
        let what = format!("Crunching {name} with {}", tool.name());
        state.result = None;
        state.job = Some(start(Some(ctx), what, move |stop| {
            let (scratch, input) = match (path, bytes) {
                (Some(path), _) => (None, path),
                (None, Some(bytes)) => {
                    let (folder, path) = scratch_copy(&name, &bytes)?;
                    (Some(folder), path)
                }
                (None, None) => return Err(crate::i18n::tr("There's nothing to crunch.").into()),
            };
            let result = tools::crunch(tool, &program, &prefs, &input, &output, stop);
            if let Some(folder) = scratch {
                let _ = std::fs::remove_dir_all(folder);
            }
            result
        }));
    }

    fn amiga_convert(&mut self, ctx: &Context) {
        let Dialog::AmigaTools(state) = &mut self.dialog else {
            return;
        };
        let Some(source) = &state.source else {
            return;
        };
        let Some(program) = found(&state.found, Tool::LspConvert) else {
            state.result = Some(Err(crate::i18n::tf!("LSPConvert wasn't found. {}", Tool::LspConvert.how_to_get())));
            return;
        };
        let Some(folder) = source.folder.clone() else {
            state.result = Some(Err(crate::i18n::tr("Choose a folder for LSPConvert's files first.").into()));
            return;
        };
        let mut options = state.prefs.lsp.clone();
        options.micro = state.replay == Replay::LspMicro;
        options.insane = state.replay == Replay::LspInsane;
        let (path, bytes, name) = (source.path.clone(), source.bytes.clone(), source.name.clone());
        let what = format!("Converting {name} with LSPConvert");
        state.result = None;
        state.job = Some(start(Some(ctx), what, move |stop| match (path, bytes) {
            (Some(path), _) => tools::convert_lsp(&program, &options, &path, stop),
            (None, Some(bytes)) => {
                // LSPConvert writes beside the module, so it works on a copy and what it makes is moved over.
                let (scratch, module) = scratch_copy(&name, &bytes)?;
                let mut outcome = tools::convert_lsp(&program, &options, &module, stop);
                if let Ok(outcome) = &mut outcome {
                    for (made, _) in &mut outcome.made {
                        let target = folder.join(made.file_name().unwrap_or_default());
                        if std::fs::copy(&*made, &target).is_ok() {
                            *made = target;
                        }
                    }
                }
                let _ = std::fs::remove_dir_all(scratch);
                outcome
            }
            (None, None) => Err(crate::i18n::tr("There's no module to convert.").into()),
        }));
    }

    /// After F9 builds an Amiga program, crunches a copy of it beside the source when the setting asks, in the
    /// background. Without the setting, or for anything that isn't an Amiga program, nothing happens; without the
    /// cruncher, the Output panel says so.
    pub(super) fn crunch_after_build(&mut self, doc_id: u64, name: &str, bytes: &[u8]) {
        if !tools::is_hunk_executable(bytes) {
            return;
        }
        let prefs = ToolPrefs::load();
        let Some(tool) = prefs.after_build else {
            return;
        };
        let Some(folder) = self.docs.iter().find(|d| d.id == doc_id).and_then(|d| d.path.as_deref()).and_then(Path::parent).map(Path::to_path_buf) else {
            return;
        };
        let Some(program) = prefs.find(tool) else {
            self.script_print(crate::i18n::tf!("Crunching after building is on, but {} wasn't found, so the program wasn't crunched (Run on, Amiga crunchers and music).", tool.name()), false);
            return;
        };
        let output = folder.join(tools::crunched_name(name));
        let (name, bytes) = (name.to_string(), bytes.to_vec());
        let what = format!("Crunching {name} with {}", tool.name());
        let job = start(None, what.clone(), move |stop| {
            let (scratch, input) = scratch_copy(&name, &bytes)?;
            let result = tools::crunch(tool, &program, &prefs, &input, &output, stop);
            let _ = std::fs::remove_dir_all(scratch);
            result
        });
        self.script_print(crate::i18n::tf!("{what}, in the background\u{2026}"), false);
        self.amiga_jobs.push(job);
    }

    /// Reports crunching after building when it's done.
    pub(super) fn amiga_jobs_tick(&mut self, ctx: &Context) {
        if self.amiga_jobs.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        let mut done = Vec::new();
        self.amiga_jobs.retain(|job| match job.rx.try_recv() {
            Ok(result) => {
                done.push(report(&job.what, &result));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
            Err(_) => true,
        });
        for lines in done {
            for line in lines {
                self.script_print(line, false);
            }
        }
    }
}

enum Act {
    Crunch,
    Convert,
    Insert(String),
    Copy(String),
    Download(Tool),
    Choose(Tool),
    Forget(Tool),
    Folder,
}

fn note(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(theme::text_dim()).size(12.0));
}

fn file_line(ui: &mut egui::Ui, source: &Source, action: &mut Option<Act>) {
    ui.horizontal_wrapped(|ui| {
        ui.strong(&source.name);
        let size = source.bytes.as_ref().map(|b| b.len() as u64).or_else(|| source.path.as_ref().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()));
        if let Some(size) = size {
            ui.label(crate::i18n::tf!("{} bytes", tools::group(size)));
        }
    });
    ui.horizontal_wrapped(|ui| {
        match &source.folder {
            Some(folder) => note(ui, format!("What's made goes in {}", folder.display())),
            None => note(ui, "Choose where what's made goes:"),
        }
        if ui.small_button(crate::i18n::tr("Choose a folder\u{2026}")).clicked() {
            *action = Some(Act::Folder);
        }
    });
}

fn tool_status(ui: &mut egui::Ui, found: &[(Tool, Option<PathBuf>)], tool: Tool) -> bool {
    match self::found(found, tool) {
        Some(path) => {
            note(ui, format!("{} is at {}", tool.name(), path.display()));
            true
        }
        None => {
            ui.colored_label(theme::rainbow_mistake(), crate::i18n::tf!("{} wasn't found.", tool.name()));
            note(ui, tool.how_to_get());
            note(ui, "Point Limoniter at it, or fetch it, on the Tools page.");
            false
        }
    }
}

fn crunch_page(ui: &mut egui::Ui, state: &mut AmigaToolsBox, action: &mut Option<Act>) {
    let Some(source) = &state.source else {
        ui.label(crate::i18n::tr("Open an Amiga program (it can be in hex), or choose one in a disk image's browser, then come back here."));
        return;
    };
    file_line(ui, source, action);
    note(ui, if source.program { "This is an Amiga program (a hunk executable)." } else { "This isn't an Amiga program, so it can only be crunched as data." });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for tool in Tool::CRUNCHERS {
            ui.radio_value(&mut state.cruncher, tool, crate::i18n::tr(tool.name()));
        }
    });
    let ready = tool_status(ui, &state.found, state.cruncher);
    ui.add_space(4.0);
    let data_default = !source.program;
    match state.cruncher {
        Tool::Shrinkler => {
            let s = &mut state.prefs.shrinkler;
            if data_default && !s.data {
                note(ui, "It isn't a program, so it can only be crunched as data (-d).");
            }
            ui.checkbox(&mut s.data, crate::i18n::tr("Data, not a program (-d)")).on_hover_text(crate::i18n::tr("Crunch raw data, to decrunch with ShrinklerDecompress.S in your own code"));
            ui.add_enabled_ui(s.data, |ui| {
                ui.checkbox(&mut s.data_header, crate::i18n::tr("Write a header with the sizes (-w)"));
            });
            ui.add_enabled_ui(!s.data, |ui| {
                ui.checkbox(&mut s.hunk_merge, crate::i18n::tr("Merge hunks of the same memory type (-h)"));
                ui.checkbox(&mut s.overlap, crate::i18n::tr("Overlap crunched and decrunched data, to save memory (-o)"));
                ui.checkbox(&mut s.mini, crate::i18n::tr("Smaller, more restricted decrunch header (-m)"));
                ui.checkbox(&mut s.command_line, crate::i18n::tr("Pass the command line on to the program (-c)"));
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Text before decrunching (-t):"));
                    ui.text_edit_singleline(&mut s.text);
                });
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Flash a register while decrunching (-f):"));
                    ui.add(egui::TextEdit::singleline(&mut s.flash).desired_width(80.0).hint_text(crate::i18n::tr("DFF180")));
                });
            });
            ui.checkbox(&mut s.bytes, crate::i18n::tr("No parity context, for byte-oriented data (-b)"));
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Effort:"));
                egui::ComboBox::from_id_salt("shrinkler-preset").selected_text(crate::i18n::tf!("-{}", s.preset)).show_ui(ui, |ui| {
                    for n in 1..=9u8 {
                        let label = match n {
                            1 => "-1 fastest".to_string(),
                            3 => "-3 Shrinkler's own default".to_string(),
                            9 => "-9 smallest (slow)".to_string(),
                            _ => format!("-{n}"),
                        };
                        ui.selectable_value(&mut s.preset, n, crate::i18n::tr(&label));
                    }
                });
                let mut iterations = s.iterations.is_some();
                if ui.checkbox(&mut iterations, crate::i18n::tr("iterations (-i)")).changed() {
                    s.iterations = iterations.then_some(u32::from(s.preset));
                }
                if let Some(n) = &mut s.iterations {
                    ui.add(egui::DragValue::new(n).range(1..=99));
                }
                let mut references = s.references.is_some();
                if ui.checkbox(&mut references, crate::i18n::tr("references (-r)")).changed() {
                    s.references = references.then_some(100_000);
                }
                if let Some(n) = &mut s.references {
                    ui.add(egui::DragValue::new(n).range(1000..=100_000_000).speed(1000));
                }
            });
        }
        Tool::Cranker => {
            let c = &mut state.prefs.cranker;
            ui.checkbox(&mut c.data, crate::i18n::tr("Data, not a program (-cd)"));
            ui.checkbox(&mut c.fast, crate::i18n::tr("Fast: crunches less (-1)"));
            ui.add_enabled_ui(!c.data, |ui| {
                ui.checkbox(&mut c.merge, crate::i18n::tr("Merge hunks (-m; Cranker calls it dangerous)"));
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("While loading (-d):"));
                    egui::ComboBox::from_id_salt("decranker").selected_text(c.decranker.clone()).show_ui(ui, |ui| {
                        for name in tools::DECRANKERS {
                            ui.selectable_value(&mut c.decranker, name.to_string(), name);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Text while loading (-t):"));
                    ui.text_edit_singleline(&mut c.text);
                });
            });
        }
        _ => {}
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Save as:"));
        ui.text_edit_singleline(&mut state.output_name);
    });
    let busy = state.job.is_some();
    if ui.add_enabled(ready && !busy, Button::new(crate::i18n::tf!("Crunch with {}", state.cruncher.name()))).clicked() {
        *action = Some(Act::Crunch);
    }
}

fn music_page(ui: &mut egui::Ui, state: &mut AmigaToolsBox, target: Option<&str>, action: &mut Option<Act>) {
    let Some(source) = &state.source else {
        ui.label(crate::i18n::tr("Open a ProTracker module (.mod, or mod.name), or choose one in a disk image's browser, then come back here."));
        return;
    };
    file_line(ui, source, action);
    let Some(module) = &source.module else {
        ui.colored_label(theme::rainbow_mistake(), crate::i18n::tr("This isn't a ProTracker module."));
        return;
    };
    note(ui, format!("{}: {} channels, {} positions, {} patterns, {} samples.", if module.title.is_empty() { "Untitled" } else { &module.title }, module.channels, module.order.len(), module.patterns, module.used_samples().count()));
    if module.channels != 4 {
        ui.colored_label(theme::rainbow_mistake(), crate::i18n::tr("Both players play 4-channel modules only."));
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Player:"));
        egui::ComboBox::from_id_salt("replay").selected_text(crate::i18n::tr(state.replay.name())).show_ui(ui, |ui| {
            for replay in Replay::ALL {
                ui.selectable_value(&mut state.replay, replay, crate::i18n::tr(replay.name()));
            }
        });
    });
    let module_path = source.folder.clone().unwrap_or_default().join(source.path.as_deref().and_then(Path::file_name).map_or(source.name.as_str().into(), |n| n.to_os_string()));
    if state.replay == Replay::P61 {
        tool_status_p61(ui, &state.found);
        let p61 = tools::p61_file(&module_path);
        if p61.is_file() {
            note(ui, format!("{} is there, ready.", p61.display()));
        } else {
            note(ui, format!("P61Con saves {} for this module.", p61.file_name().unwrap_or_default().to_string_lossy()));
        }
    } else {
        let ready = tool_status(ui, &state.found, Tool::LspConvert);
        let l = &mut state.prefs.lsp;
        ui.horizontal_wrapped(|ui| {
            let micro = state.replay == Replay::LspMicro;
            let insane = state.replay == Replay::LspInsane;
            ui.add_enabled(!micro, egui::Checkbox::new(&mut l.getpos, "-getpos")).on_hover_text(crate::i18n::tr("LSP_MusicGetPos can say where in the song it is"));
            ui.add_enabled(!micro && !insane, egui::Checkbox::new(&mut l.setpos, "-setpos")).on_hover_text(crate::i18n::tr("LSP_MusicSetPos can jump to a position"));
            ui.checkbox(&mut l.shrink, crate::i18n::tr("-shrink")).on_hover_text(crate::i18n::tr("Leave out sample data that's never played"));
            ui.checkbox(&mut l.adpcm, crate::i18n::tr("-adpcm")).on_hover_text(crate::i18n::tr("A sample bank that packs to less than half (decoded when LSP starts)"));
            ui.checkbox(&mut l.fixed50hz, crate::i18n::tr("-fixed50hz")).on_hover_text(crate::i18n::tr("Plays at 50 Hz whatever the BPM, with no CIA timer"));
            ui.checkbox(&mut l.nosettempo, crate::i18n::tr("-nosettempo")).on_hover_text(crate::i18n::tr("For very old modules: Fxx above $20 isn't a tempo"));
            ui.checkbox(&mut l.nosampleoptim, crate::i18n::tr("-nosampleoptim")).on_hover_text(crate::i18n::tr("Keep the module's own sample layout"));
            ui.checkbox(&mut l.preview_wav, crate::i18n::tr("-amigapreview")).on_hover_text(crate::i18n::tr("Also write a WAV of how LSP will sound on the Amiga"));
        });
        note(ui, "LSPConvert writes the .lsmusic (the score) and .lsbank (the samples, for chip memory) beside the module, and for insane mode the player's source too.");
        if ui.add_enabled(ready && state.job.is_none(), Button::new(crate::i18n::tr("Convert with LSPConvert"))).clicked() {
            *action = Some(Act::Convert);
        }
    }
    ui.add_space(6.0);
    ui.strong("Replay calls");
    note(ui, "Written by Limoniter for you to adapt: they call the player's routines and include its files, from its own package.");
    let snippet = tools::replay_snippet(state.replay, &module_path);
    let mut shown = snippet.as_str();
    ui.add(egui::TextEdit::multiline(&mut shown).code_editor().desired_rows(10).desired_width(f32::INFINITY));
    ui.horizontal(|ui| {
        match target {
            Some(name) => {
                if ui.button(crate::i18n::tf!("Insert into {name}")).on_hover_text(crate::i18n::tr("At the cursor, as one change to undo")).clicked() {
                    *action = Some(Act::Insert(snippet.clone()));
                }
            }
            None => note(ui, "Open 68000 source to insert them into it."),
        }
        if ui.button(crate::i18n::tr("Copy")).clicked() {
            *action = Some(Act::Copy(snippet.clone()));
        }
    });
}

fn tool_status_p61(ui: &mut egui::Ui, found: &[(Tool, Option<PathBuf>)]) {
    ui.colored_label(theme::rainbow_mistake(), crate::i18n::tr("P61Con runs only on an Amiga."));
    note(ui, Tool::P61Con.how_to_get());
    if let Some(path) = self::found(found, Tool::P61Con) {
        note(ui, format!("There's a copy at {}, to copy to the Amiga (or the emulator's hard disk folder).", path.display()));
    }
}

fn tools_page(ui: &mut egui::Ui, state: &mut AmigaToolsBox, action: &mut Option<Act>) {
    note(ui, "Limoniter runs these where they're installed, or where you point it. It never fetches one unless you ask.");
    ui.add_space(4.0);
    for tool in Tool::ALL {
        ui.horizontal_wrapped(|ui| {
            ui.strong(tool.name());
            ui.hyperlink_to("website", tool.website());
        });
        note(ui, tool.licence());
        if !tool.runs_here() {
            note(ui, "An Amiga program: Limoniter can find it for you to copy to an Amiga, but can't run it here.");
        }
        match found(&state.found, tool) {
            Some(path) => note(ui, format!("Found: {}", path.display())),
            None => note(ui, format!("Not found. {}", tool.how_to_get())),
        }
        ui.horizontal(|ui| {
            if ui.small_button(crate::i18n::tr("Choose\u{2026}")).on_hover_text(crate::i18n::tr("Point Limoniter at the program")).clicked() {
                *action = Some(Act::Choose(tool));
            }
            if state.prefs.path(tool).is_some() && ui.small_button(crate::i18n::tr("Forget")).on_hover_text(crate::i18n::tr("Go back to looking where it's usually installed")).clicked() {
                *action = Some(Act::Forget(tool));
            }
            if tool.release().is_some() && ui.add_enabled(state.downloading.is_none(), Button::new(crate::i18n::tr("Download its release")).small()).on_hover_text(tool.release().map_or("", |r| r.0)).clicked() {
                *action = Some(Act::Download(tool));
            }
        });
        ui.add_space(6.0);
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("After F9 builds an Amiga program, crunch a copy with:"));
        egui::ComboBox::from_id_salt("after-build").selected_text(state.prefs.after_build.map_or("Nothing", Tool::name)).show_ui(ui, |ui| {
            ui.selectable_value(&mut state.prefs.after_build, None, crate::i18n::tr("Nothing"));
            for tool in Tool::CRUNCHERS {
                ui.selectable_value(&mut state.prefs.after_build, Some(tool), tool.name());
            }
        });
    });
    note(ui, "The copy, crunched with the options on the Crunch page, goes beside the source as name_crunched, in the background; the program runs as built.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{app_with, draw};

    fn file(name: &str, bytes: &[u8]) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("limoniter-amiga-host-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn the_box(app: &mut Limoniter) -> &mut AmigaToolsBox {
        match &mut app.dialog {
            Dialog::AmigaTools(state) => state,
            _ => panic!("the box isn't open"),
        }
    }

    fn program(longs: &[u32]) -> Vec<u8> {
        let mut bytes: Vec<u8> = [0x3F3u32, 0, 1, 0, 0, longs.len() as u32, 0x3E9, longs.len() as u32].iter().flat_map(|w| w.to_be_bytes()).collect();
        bytes.extend(longs.iter().flat_map(|w| w.to_be_bytes()));
        bytes.extend(0x3F2u32.to_be_bytes());
        bytes
    }

    #[test]
    fn a_module_opens_with_its_details_beside_it_and_its_replay_calls_go_into_68000_source() {
        let (mut app, ctx) = app_with("");
        crate::highlight::load_now();
        let source = file("demo.s68", b"\tsection\tcode,code\nstart:\tmoveq\t#0,d0\n\trts\n");
        assert!(app.open_path(&source));
        draw(&mut app, &ctx, Vec::new());
        let source_tab = app.active;
        let module = crate::protracker::tests::synthetic("my tune", &[("lead", 100, 64, 0, 0, 1)], &[0, 0, 1], b"M.K.", 4);
        let path = file("mod.mytune", &module);
        assert!(app.open_path(&path));
        assert!(app.docs[app.active].hex_mode);
        assert_eq!(app.docs[app.active].preview_view, crate::settings::PreviewView::Split, "a module opens with its details showing");
        draw(&mut app, &ctx, Vec::new());
        assert!(app.preview_blocks.iter().any(|b| b.spans.first().is_some_and(|s| s.text == "my tune")), "the preview shows the module");
        app.open_amiga_tools();
        draw(&mut app, &ctx, Vec::new());
        let state = the_box(&mut app);
        assert!(state.page == Page::Music);
        assert_eq!(state.source.as_ref().and_then(|s| s.module.as_ref()).map(|m| m.patterns), Some(2));
        assert_eq!(app.m68k_target(), Some(source_tab), "the 68000 source open in another tab is where the calls go");
        let snippet = tools::replay_snippet(Replay::LspCopper, &path);
        app.amiga_tools_act(&ctx, Act::Insert(snippet));
        assert_eq!(app.active, source_tab);
        let text = app.docs[source_tab].text.all_lines().join("\n");
        assert!(text.contains("bsr\tLSP_MusicPlayTick") && text.contains("incbin\t\"mytune.lsbank\""), "{text}");
    }

    #[test]
    fn a_program_opens_the_crunch_page_and_a_missing_cruncher_is_only_a_note() {
        let (mut app, ctx) = app_with("");
        let bytes = program(&[0x7000_4E75]);
        let path = file("demo", &bytes);
        assert!(app.open_path(&path));
        app.open_amiga_tools();
        draw(&mut app, &ctx, Vec::new());
        let state = the_box(&mut app);
        assert!(state.page == Page::Crunch);
        assert!(state.source.as_ref().is_some_and(|s| s.program));
        assert_eq!(state.output_name, "demo_crunched");
        // A cruncher that isn't there says how to get it, and nothing runs.
        state.found = Tool::ALL.into_iter().map(|t| (t, None)).collect();
        app.amiga_crunch(&ctx);
        let state = the_box(&mut app);
        assert!(state.job.is_none());
        assert!(matches!(&state.result, Some(Err(problem)) if problem.contains("wasn't found")));
        // After building, with no cruncher chosen, nothing happens at all.
        let id = app.docs[app.active].id;
        app.crunch_after_build(id, "demo", &bytes);
        assert!(app.amiga_jobs.is_empty());
    }

    /// With Shrinkler named by LIMONITER_SHRINKLER, the whole way: the box crunches the program in the background,
    /// and F9's build is crunched beside its source when the setting says.
    #[test]
    fn shrinkler_crunches_from_the_box_and_after_building() {
        let Some(shrinkler) = std::env::var_os("LIMONITER_SHRINKLER").map(PathBuf::from).filter(|p| p.is_file()) else {
            eprintln!("LIMONITER_SHRINKLER isn't set, so nothing was crunched.");
            return;
        };
        let (mut app, ctx) = app_with("");
        let longs: Vec<u32> = (0..1500u32).map(|i| 0x303C_0000 | (i % 13)).collect();
        let bytes = program(&longs);
        let path = file("intro", &bytes);
        assert!(app.open_path(&path));
        app.open_amiga_tools();
        let state = the_box(&mut app);
        state.prefs.set_path(Tool::Shrinkler, Some(shrinkler.clone()));
        state.prefs.shrinkler.preset = 1;
        state.found = find_all(&state.prefs);
        app.amiga_crunch(&ctx);
        let started = std::time::Instant::now();
        while the_box(&mut app).job.is_some() && started.elapsed().as_secs() < 60 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            draw(&mut app, &ctx, Vec::new());
        }
        let outcome = the_box(&mut app).result.clone().unwrap().unwrap();
        assert!(outcome.ok, "{:?}", outcome.printed);
        let crunched = path.with_file_name("intro_crunched");
        assert!(tools::is_hunk_executable(&std::fs::read(&crunched).unwrap()));
        assert!(app.script_output.iter().any(|l| l.contains("Crunched in")), "{:?}", app.script_output);
        // The setting, kept as the box left it, then F9's build.
        let mut prefs = ToolPrefs::load();
        assert_eq!(prefs.path(Tool::Shrinkler), Some(shrinkler.as_path()), "the box remembered where Shrinkler is");
        prefs.after_build = Some(Tool::Shrinkler);
        prefs.save();
        app.dialog = Dialog::None;
        let _ = std::fs::remove_file(&crunched);
        let id = app.docs[app.active].id;
        app.crunch_after_build(id, "intro", &bytes);
        assert_eq!(app.amiga_jobs.len(), 1);
        while !app.amiga_jobs.is_empty() && started.elapsed().as_secs() < 120 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            app.amiga_jobs_tick(&ctx);
        }
        assert!(tools::is_hunk_executable(&std::fs::read(&crunched).unwrap()), "{:?}", app.script_output);
    }
}
