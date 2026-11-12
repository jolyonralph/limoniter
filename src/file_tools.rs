//! Files and the world outside the text: switching between files in the order they were used, saving part of a file
//! or splitting one into several, renaming a file and showing it in Explorer, reusable snippets, running other
//! programs on a file, spell checking, and files handed over by a second Limoniter.

use super::*;
use crate::spelling::{self, Ask, Told};
use std::io::BufRead;
use std::sync::atomic::AtomicUsize;

/// The most a split can make, so a mistyped number can't fill a folder.
const MOST_PARTS: usize = 10_000;
const MOST_RECENT_RUNS: usize = 15;
const MOST_TOOL_LINES: usize = 100_000;

// ---------- Snippets ----------

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Snippet {
    pub name: String,
    pub body: String,
}

/// What's offered before any have been made, to show how they work.
fn starter_snippets() -> Vec<Snippet> {
    vec![
        Snippet { name: "Dated heading".into(), body: "## $DATE\n\n$CURSOR".into() },
        Snippet { name: "In quotes".into(), body: "\u{201C}$SELECTION$CURSOR\u{201D}".into() },
        Snippet { name: "Note with time".into(), body: "Note ($DATE $TIME): $CURSOR".into() },
    ]
}

/// A snippet's text on one line of the settings file.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

pub(super) fn snippets_from_settings(settings: &str) -> Vec<Snippet> {
    if !settings.lines().any(|line| line.trim() == "snippets_set=true") {
        return starter_snippets();
    }
    list_setting(settings, "snippet")
        .iter()
        .filter_map(|line| {
            let (name, body) = line.split_once('\t')?;
            Some(Snippet { name: unescape(name), body: unescape(body) })
        })
        .collect()
}

pub(super) fn snippets_to_settings(snippets: &[Snippet]) -> String {
    let mut out = "snippets_set=true\n".to_string();
    for snippet in snippets {
        out += &format!("snippet={}\t{}\n", escape(&snippet.name.replace(['\t', '\n', '\r'], " ")), escape(&snippet.body));
    }
    out
}

/// What fills a snippet's blanks.
pub(super) struct Fill<'a> {
    pub date: &'a str,
    pub time: &'a str,
    pub file: &'a str,
    pub selection: &'a str,
}

/// A snippet with its blanks filled: the text before where the cursor goes, and after it.
pub(super) fn expand_snippet(body: &str, fill: &Fill) -> (String, String) {
    let filled = body.replace("$DATE", fill.date).replace("$TIME", fill.time).replace("$FILE", fill.file).replace("$SELECTION", fill.selection);
    match filled.split_once("$CURSOR") {
        Some((before, after)) => (before.to_string(), after.replace("$CURSOR", "")),
        None => (filled, String::new()),
    }
}

#[derive(Default)]
pub(super) struct SnippetBox {
    pub filter: String,
    /// Which snippet, by where it is in the whole list.
    pub chosen: Option<usize>,
    pub focus: bool,
}

// ---------- Splitting a file ----------

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SplitBy {
    Lines(usize),
    Containing(String),
}

/// Where each part starts, by line. A last line that's empty is only the end of the one before, so it never makes a
/// part of its own.
pub(super) fn part_starts(count: usize, line: &dyn Fn(usize) -> Arc<str>, by: &SplitBy) -> Vec<usize> {
    let usable = if count > 1 && line(count - 1).is_empty() { count - 1 } else { count };
    match by {
        SplitBy::Lines(every) => (0..usable).step_by((*every).max(1)).collect(),
        SplitBy::Containing(text) => std::iter::once(0).chain((1..usable).filter(|at| line(*at).contains(text.as_str()))).collect(),
    }
}

/// The parts' names, beside the file: notes.txt makes notes.part-001.txt and so on.
pub(super) fn part_names(path: &Path, count: usize) -> Vec<PathBuf> {
    let stem = path.file_stem().map_or_else(|| "part".to_string(), |stem| stem.to_string_lossy().into_owned());
    let extension = path.extension().map(|ext| format!(".{}", ext.to_string_lossy())).unwrap_or_default();
    let width = count.to_string().len().max(3);
    (1..=count).map(|n| path.with_file_name(format!("{stem}.part-{n:0width$}{extension}"))).collect()
}

/// Writes the parts. Nothing is written if any of them is there already.
fn write_parts(store: &crate::line_store::LineStore, format: &TextFormat, path: &Path, by: &SplitBy, cancel: &AtomicBool) -> Result<Vec<PathBuf>, String> {
    let count = store.len();
    let starts = part_starts(count, &|at| store.get(at), by);
    if starts.len() > MOST_PARTS {
        return Err(format!("That would make {} files. The most is {MOST_PARTS}.", starts.len()));
    }
    if starts.len() < 2 {
        return Err("That would leave the file in one piece, so nothing was written.".into());
    }
    let names = part_names(path, starts.len());
    if let Some(there) = names.iter().find(|name| name.exists()) {
        return Err(format!("{} is there already, so nothing was written.", there.display()));
    }
    let progress = AtomicU64::new(0);
    for (index, name) in names.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(format!("Stopped after {index} files."));
        }
        let start = starts[index];
        let end = starts.get(index + 1).copied().unwrap_or(count);
        let mut part = store.extract(start, end - 1);
        // Each part keeps the line ending it had in the file, so joined up again they make the file exactly.
        if end < count {
            part.push(Arc::from(""));
        }
        part.write_file(name, format, &progress).map_err(|e| format!("Couldn't write {}.\n\n{e}", name.display()))?;
    }
    Ok(names)
}

pub(super) struct SplitJob {
    result: Arc<Mutex<Option<Result<Vec<PathBuf>, String>>>>,
    cancel: Arc<AtomicBool>,
}

// ---------- Running other programs ----------

/// A command line with the file's details put in.
pub(super) fn expand_run_line(line: &str, file: Option<&Path>) -> Result<String, String> {
    let line = line.trim();
    if line.is_empty() {
        return Err("Type a command to run.".into());
    }
    let needs_file = ["%FILE%", "%NAME%", "%DIR%", "%STEM%"].iter().any(|blank| line.contains(blank));
    let Some(file) = file else {
        return if needs_file { Err("This file hasn't been saved yet, so there's nothing for %FILE%, %NAME%, %DIR% or %STEM% to stand for.".into()) } else { Ok(line.to_string()) };
    };
    let text = |part: Option<&std::ffi::OsStr>| part.map_or_else(String::new, |p| p.to_string_lossy().into_owned());
    Ok(line
        .replace("%FILE%", &file.display().to_string())
        .replace("%NAME%", &text(file.file_name()))
        .replace("%STEM%", &text(file.file_stem()))
        .replace("%DIR%", &file.parent().map_or_else(String::new, |dir| dir.display().to_string())))
}

pub(super) struct RunJob {
    lines: Arc<Mutex<Vec<String>>>,
    taken: usize,
    process: u32,
    finished: Arc<Mutex<Option<String>>>,
    stopped: Arc<AtomicBool>,
}

fn read_into(stream: impl std::io::Read, lines: Arc<Mutex<Vec<String>>>, dropped: Arc<AtomicUsize>) {
    let mut reader = std::io::BufReader::new(stream);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                while bytes.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
                    bytes.pop();
                }
                let mut shared = lines.lock().unwrap();
                if shared.len() < MOST_TOOL_LINES {
                    shared.push(String::from_utf8_lossy(&bytes).into_owned());
                } else {
                    dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

/// Runs a command line as the Command Prompt would, with no window of its own, in this folder.
pub(super) fn start_run(line: &str, folder: Option<&Path>, wake: impl Fn() + Send + 'static) -> Result<RunJob, String> {
    let mut command = crate::platform::shell_command(line);
    if let Some(folder) = folder.filter(|folder| folder.is_dir()) {
        command.current_dir(folder);
    }
    let mut child = command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().map_err(|e| format!("Couldn't run it.\n\n{e}"))?;
    let lines = Arc::new(Mutex::new(Vec::new()));
    let dropped = Arc::new(AtomicUsize::new(0));
    let readers: Vec<_> = [child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)]
        .into_iter()
        .flatten()
        .map(|stream| {
            let (lines, dropped) = (lines.clone(), dropped.clone());
            std::thread::spawn(move || read_into(stream, lines, dropped))
        })
        .collect();
    let finished = Arc::new(Mutex::new(None));
    let stopped = Arc::new(AtomicBool::new(false));
    let process = child.id();
    let (done, was_stopped) = (finished.clone(), stopped.clone());
    std::thread::spawn(move || {
        let status = child.wait();
        for reader in readers {
            let _ = reader.join();
        }
        let extra = dropped.load(Ordering::Relaxed);
        let mut summary = match status {
            _ if was_stopped.load(Ordering::Relaxed) => "Stopped.".to_string(),
            Ok(status) => match status.code() {
                Some(0) => "Finished.".to_string(),
                Some(code) => format!("Finished, with exit code {code}."),
                None => "Finished.".to_string(),
            },
            Err(e) => format!("Couldn't wait for it to finish: {e}"),
        };
        if extra > 0 {
            summary += &format!(" {extra} more lines weren't kept.");
        }
        *done.lock().unwrap() = Some(summary);
        wake();
    });
    Ok(RunJob { lines, taken: 0, process, finished, stopped })
}

impl RunJob {
    /// Ends the command, and anything it started.
    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        crate::platform::kill_tree(self.process);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SpellAnswer {
    Change(String),
    Skip,
    Ignore,
    Add,
}

// ---------- Opening things with Windows ----------

/// Opens a file the way double-clicking it in Explorer would.
#[cfg(windows)]
fn shell_open(path: &Path) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR, w};
    // SAFETY: every string lives until the call returns.
    let result = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(path.as_os_str()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    if result.0 as isize > 32 { Ok(()) } else { Err("Windows has no program set to open this kind of file. One can be chosen in Explorer, with Open with.".into()) }
}

#[cfg(not(windows))]
fn shell_open(path: &Path) -> Result<(), String> {
    crate::platform::open_with_default(path)
}

/// A listing tokenised as the file a machine loads: its bytes, its file ending, and what it is in words.
pub(super) fn tokenised<S: AsRef<str>>(lines: &[S], machine: basic::Machine, stem: &str, run_from: Option<u16>) -> Result<(Vec<u8>, &'static str, &'static str), String> {
    let bytes = match machine {
        basic::Machine::Spectrum => basic::tokenise_spectrum(lines, basic::ZxDialect::Spectrum).map(|program| basic::spectrum_tape(&program, stem, run_from)),
        basic::Machine::Next => basic::tokenise_spectrum(lines, basic::ZxDialect::Next).map(|program| basic::plus3dos_file(&program, run_from)),
        basic::Machine::Commodore(cbm) => basic::tokenise_commodore(lines, cbm),
        basic::Machine::Bbc => crate::basic_more::tokenise_bbc(lines),
    }?;
    Ok((bytes, machine.ending(), machine.kind()))
}

impl Limoniter {
    // ---------- Files handed over by a second Limoniter ----------

    /// Listens for files opened from Explorer while this window is open.
    pub(super) fn start_listening(&mut self, ctx: &Context) {
        if cfg!(test) || self.screenshot.is_some() {
            return;
        }
        if let Some(folder) = settings_folder() {
            let ctx = ctx.clone();
            self.listening = crate::instance::listen(&folder, move || ctx.request_repaint());
        }
    }

    pub(super) fn open_handed_over(&mut self, ctx: &Context) {
        let Some(listening) = &self.listening else {
            return;
        };
        let mut files = Vec::new();
        while let Ok(more) = listening.files.try_recv() {
            files.extend(more);
        }
        if files.is_empty() {
            return;
        }
        // While a script runs, the file in front has to stay in front, so these wait their turn.
        if self.script.is_some() {
            self.handed_over.extend(files);
            return;
        }
        for file in std::mem::take(&mut self.handed_over).into_iter().chain(files) {
            self.open_path(&file);
        }
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    // ---------- Switching by the order files were used ----------

    /// Puts the file in front first in the order of use, once a Ctrl+Tab switch has been let go of.
    pub(super) fn note_file_use(&mut self, ctx: &Context) {
        if self.cycle.is_some() && !ctx.input(|i| i.modifiers.ctrl) && !self.hold_cycle {
            self.cycle = None;
        }
        if self.cycle.is_some() {
            return;
        }
        let open: Vec<u64> = self.docs.iter().filter(|doc| !doc.placeholder).map(|doc| doc.id).collect();
        self.used_order.retain(|id| open.contains(id));
        // Files not seen yet go after those used, newest first.
        for id in open.iter().rev() {
            if !self.used_order.contains(id) {
                self.used_order.push(*id);
            }
        }
        if let Some(doc) = self.docs.get(self.active).filter(|doc| !doc.placeholder) {
            let id = doc.id;
            self.used_order.retain(|other| *other != id);
            self.used_order.insert(0, id);
        }
    }

    /// One step back through the files in the order they were used, or forward again. Holding Ctrl keeps going.
    pub(super) fn cycle_used(&mut self, back: bool) {
        let (order, position) = match self.cycle.take() {
            Some(cycle) => cycle,
            None => (self.used_order.clone(), 0),
        };
        let order: Vec<u64> = order.into_iter().filter(|id| self.docs.iter().any(|doc| doc.id == *id)).collect();
        if order.len() < 2 {
            return;
        }
        let next = if back { (position + 1) % order.len() } else { (position + order.len() - 1) % order.len() };
        if let Some(index) = self.docs.iter().position(|doc| doc.id == order[next]) {
            self.active = index;
            self.pending_focus = true;
        }
        self.cycle = Some((order, next));
    }

    /// While Ctrl is held, the files in the order they were used, with the one being gone to marked.
    pub(super) fn draw_cycle(&self, ctx: &Context) {
        let Some((order, position)) = &self.cycle else {
            return;
        };
        if !ctx.input(|i| i.modifiers.ctrl) && !self.hold_cycle {
            return;
        }
        egui::Area::new(egui::Id::unique("used-files")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_CENTER, [0.0, -40.0]).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_min_width(320.0);
                ui.label(RichText::new("Files, most recently used first").color(theme::text_faint()).size(11.5));
                ui.add_space(4.0);
                for (index, id) in order.iter().enumerate() {
                    let Some(doc) = self.docs.iter().find(|doc| doc.id == *id) else {
                        continue;
                    };
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::hover());
                    if index == *position {
                        ui.painter().rect_filled(rect, 6.0, theme::selected());
                        ui.painter().rect_filled(egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 5.0), egui::vec2(3.0, 16.0)), 1.5, theme::accent());
                    }
                    ui.painter().text(rect.left_center() + egui::vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, doc.name(), egui::FontId::proportional(14.0), theme::text());
                    if let Some(folder) = doc.path.as_deref().and_then(Path::parent) {
                        ui.painter().text(rect.right_center() - egui::vec2(10.0, 0.0), egui::Align2::RIGHT_CENTER, folder.display().to_string(), egui::FontId::proportional(11.5), theme::text_faint());
                    }
                }
            });
        });
    }

    // ---------- Swap with clipboard ----------

    pub(super) fn swap_with_clipboard(&mut self, ui: &Ui) {
        let text = match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
            Ok(text) => text,
            Err(arboard::Error::ContentNotAvailable) => String::new(),
            Err(e) => return warn(&format!("Couldn't read the clipboard.\n\n{e}")),
        };
        self.swap_in(&text, ui);
    }

    /// The selection goes on the clipboard, and what was there takes its place.
    pub(super) fn swap_in(&mut self, text: &str, ui: &Ui) {
        if text.is_empty() {
            self.message("Swap with clipboard", "There's no text on the clipboard to swap in.".into());
            return;
        }
        let doc = &mut self.docs[self.active];
        if doc.hex_mode {
            if doc.hex.has_selection() {
                views::copy_hex_selection(ui, &doc.hex);
                doc.hex.paste_text(text);
            }
        } else if doc.text.has_selection() || doc.text.has_multi() {
            views::paste_swapping(ui.ctx(), &mut doc.text, &mut self.clip, text, true);
        }
    }

    // ---------- Save selection, and split ----------

    /// The selection as it would be saved: in the file's encoding, with its kind of line ending.
    pub(super) fn selection_bytes(&self) -> Result<Vec<u8>, String> {
        let doc = &self.docs[self.active];
        let bytes = if doc.hex_mode {
            let (a, z) = (doc.hex.caret.min(doc.hex.anchor), doc.hex.caret.max(doc.hex.anchor));
            if z - a > crate::codec::MAX_FILE_BYTES as usize {
                return Err("That selection is too large to save this way. Split into parts can write a file of any size in pieces.".into());
            }
            doc.hex.read(a, z - a)
        } else {
            if doc.text.selection_too_large_to_copy() {
                return Err("That selection is too large to save this way. Split into parts can write a file of any size in pieces.".into());
            }
            let text = doc.text.selection_text();
            let lines: Vec<&str> = text.split("\r\n").collect();
            let eol = doc.text.insert_eol();
            let format = TextFormat { has_bom: false, ..doc.text.format.clone() };
            let mut bytes = doc.text.format.charset.bom().filter(|_| doc.text.format.has_bom && !text.is_empty()).map(<[u8]>::to_vec).unwrap_or_default();
            bytes.extend(codec::encode(&lines, &|_| eol, &format).map_err(|problems| format!("{} character(s) in the selection can't be written in {}. Change the encoding, or select less.", problems.total, doc.text.format.encoding_name()))?);
            bytes
        };
        if bytes.is_empty() {
            return Err("Select something to save first.".into());
        }
        Ok(bytes)
    }

    pub(super) fn save_selection(&mut self) {
        let bytes = match self.selection_bytes() {
            Ok(bytes) => bytes,
            Err(problem) => return self.message("Save the selection", problem),
        };
        let doc = &self.docs[self.active];
        let extension = doc.path.as_deref().and_then(Path::extension).map(|ext| format!(".{}", ext.to_string_lossy())).unwrap_or_else(|| ".txt".into());
        let mut picker = rfd::FileDialog::new().set_title("Save the selection as").set_file_name(format!("selection{extension}"));
        if let Some(folder) = doc.path.as_deref().and_then(Path::parent) {
            picker = picker.set_directory(folder);
        }
        if let Some(path) = picker.save_file() {
            self.write_selection(&path, &bytes);
        }
    }

    /// The document's exact bytes, from hex mode or the text, for the BASIC commands.
    pub(super) fn document_bytes(&self) -> Vec<u8> {
        let doc = &self.docs[self.active];
        if doc.hex_mode { doc.hex.read(0, crate::codec::MAX_FILE_BYTES as usize) } else { doc.text.encode(&doc.text.format, Lossy::Omit).unwrap_or_default() }
    }

    /// Saves the picture this file holds as a PNG, beside the file if it has a place on disk.
    pub(super) fn save_picture_as_png(&mut self) {
        let doc = &self.docs[self.active];
        // A file opened out of an image is named for where it came from; its own name is the last part.
        let whole = doc.path_or_name().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let name = whole.rsplit('\u{203A}').next().unwrap_or(&whole).trim().to_string();
        let folder = doc.path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        let bytes = self.document_bytes();
        let png = crate::pictures::detect(&name, &bytes)
            .ok_or_else(|| "This isn't a picture Limoniter can read. Amiga IFF, Atari ST (Degas, NEOchrome), Commodore 64 (Koala, Art Studio, Doodle!), MacPaint, RISC OS sprite, PCX, GEM IMG, Kodak Photo CD and ZX Spectrum screen pictures can be saved as PNG.".to_string())
            .and_then(|format| crate::pictures::decode_named(format, &name, &bytes))
            .and_then(|picture| crate::pictures::to_png(&picture));
        let png = match png {
            Ok(png) => png,
            Err(problem) => return self.message("Save as PNG", problem),
        };
        let mut picker = rfd::FileDialog::new().set_title("Save the picture as PNG").set_file_name(crate::pictures::png_name(&name)).add_filter("PNG picture", &["png"]);
        if let Some(folder) = folder {
            picker = picker.set_directory(folder);
        }
        if let Some(target) = picker.save_file()
            && let Err(e) = codec::write_file(&target, &png)
        {
            self.message("Save as PNG", format!("Couldn't save {}.\n\n{e}", target.display()));
        }
    }

    /// Lists a tokenised BASIC program into a new document, coloured as BASIC, to read or edit and save back.
    pub(super) fn open_basic_listing(&mut self) {
        let doc = &self.docs[self.active];
        let extension = doc.path_or_name().extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
        let bytes = self.document_bytes();
        if !basic::is_program_file(&extension, Some(&bytes)) {
            return self.message("BASIC listing", "This isn't a tokenised BASIC program. Commodore .prg and .p00 files, Spectrum .tap and .tzx tapes, +3DOS and Spectrum Next .bas files, and BBC, MSX and Atari BASIC programs can be listed.".into());
        }
        let listing = basic::list(&bytes, &extension, doc.text.format.charset);
        if listing.lines.is_empty() {
            let why = if listing.notes.is_empty() { "There's no BASIC program in this file.".to_string() } else { listing.notes.join("\n") };
            return self.message("BASIC listing", why);
        }
        let stem = doc.path.as_deref().and_then(Path::file_stem).map_or_else(|| doc.name(), |s| s.to_string_lossy().into_owned());
        // Only the Commodore and Sinclair dialects have a grammar of their own; the others are left plain.
        let grammar = match listing.dialect {
            d if d.contains("Commodore") => Some(highlight::BASIC_COMMODORE),
            d if d.contains("Spectrum") => Some(highlight::BASIC_SINCLAIR),
            _ => None,
        };
        let mut listed = Document::untitled(format!("{stem} listing"), &PRESETS[0]);
        listed.text.set_content(listing.lines.clone());
        listed.listing_of = basic::machine_of(listing.dialect, &extension, &bytes).map(|machine| (machine, stem.clone()));
        if let Some((_, syntax)) = grammar.and_then(|grammar| highlight::engine().and_then(|engine| engine.names().iter().find(|(name, _)| name == grammar))) {
            listed.set_language(Language::Named(*syntax));
        }
        if self.no_file() {
            self.docs.clear();
        }
        self.docs.push(listed);
        self.active = self.docs.len() - 1;
        if !listing.notes.is_empty() {
            self.message("BASIC listing", listing.notes.join("\n"));
        }
    }

    /// Tokenises the text as a BASIC program for a machine, and saves it as the file that machine loads.
    pub(super) fn save_tokenised(&mut self, machine: basic::Machine, run_from: Option<u16>) {
        let doc = &self.docs[self.active];
        let lines = doc.text.all_lines();
        let stem = doc.path.as_deref().and_then(Path::file_stem).map_or_else(|| doc.name().replace(" listing", ""), |s| s.to_string_lossy().replace(" listing", ""));
        let made = tokenised(&lines, machine, &stem, run_from);
        let (bytes, extension, kind) = match made {
            Ok(made) => made,
            Err(problem) => return self.message("Tokenise BASIC", format!("The program couldn't be tokenised, so nothing was saved.\n\n{problem}")),
        };
        let mut picker = rfd::FileDialog::new().set_title(format!("Save as a {kind}")).set_file_name(format!("{stem}.{extension}")).add_filter(kind, &[extension]);
        if let Some(folder) = doc.path.as_deref().and_then(Path::parent) {
            picker = picker.set_directory(folder);
        }
        if let Some(path) = picker.save_file() {
            match codec::write_file(&path, &bytes) {
                Ok(()) => panels::note_recent(&mut self.panels.recent, &path),
                Err(e) => self.message("Tokenise BASIC", format!("Couldn't save {}.\n\n{e}", path.display())),
            }
        }
    }

    pub(super) fn write_selection(&mut self, path: &Path, bytes: &[u8]) {
        match codec::write_file(path, bytes) {
            Ok(()) => panels::note_recent(&mut self.panels.recent, path),
            Err(e) => self.message("Save the selection", format!("Couldn't save {}.\n\n{e}", path.display())),
        }
    }

    pub(super) fn begin_split(&mut self) {
        if self.docs[self.active].path.is_none() {
            self.message("Split into parts", "Save this file first. The parts are written beside it.".into());
            return;
        }
        if self.split_job.is_some() {
            self.message("Split into parts", "A split is already being written.".into());
            return;
        }
        self.dialog = Dialog::SplitFile { every: "1000".into(), by_text: false, text: String::new(), error: String::new() };
    }

    pub(super) fn start_split(&mut self, by: SplitBy) {
        let doc = &self.docs[self.active];
        let Some(path) = doc.path.clone() else {
            return;
        };
        let (store, format) = (doc.text.store().clone(), doc.text.format.clone());
        let result = Arc::new(Mutex::new(None));
        let cancel = Arc::new(AtomicBool::new(false));
        let (out, stop) = (result.clone(), cancel.clone());
        std::thread::spawn(move || {
            let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| write_parts(&store, &format, &path, &by, &stop)));
            *out.lock().unwrap() = Some(attempt.unwrap_or_else(|_| Err("Splitting failed unexpectedly.".into())));
        });
        self.split_job = Some(SplitJob { result, cancel });
    }

    /// Says how a split went, once it's finished. True while it's still going.
    pub(super) fn check_split(&mut self) -> bool {
        let Some(job) = &self.split_job else {
            return false;
        };
        let Some(result) = job.result.lock().unwrap().take() else {
            return true;
        };
        self.split_job = None;
        let body = match result {
            Ok(names) => {
                let name = |path: &PathBuf| path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                format!("Written as {} files beside it, from {} to {}, as the file is now, with any unsaved changes.", names.len(), name(&names[0]), name(&names[names.len() - 1]))
            }
            Err(problem) => problem,
        };
        self.message("Split into parts", body);
        false
    }

    pub(super) fn stop_split(&self) {
        if let Some(job) = &self.split_job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    // ---------- Rename, Explorer, and the usual program ----------

    pub(super) fn begin_rename(&mut self) {
        let doc = &self.docs[self.active];
        let Some(path) = &doc.path else {
            return;
        };
        if doc.text.is_disk_backed() && !doc.hex_mode {
            self.message("Rename", "This file is so large that it's read from the disk as it's shown, so it can't be renamed while it's open.".into());
            return;
        }
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        self.dialog = Dialog::Rename { name, error: String::new(), focus: true };
    }

    /// Renames the file in front, in its own folder. Says why if it can't.
    pub(super) fn rename_to(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Type the new name.".into());
        }
        if let Some(bad) = name.chars().find(|c| r#"\/:*?"<>|"#.contains(*c) || c.is_control()) {
            return Err(format!("A file name can't have {bad} in it."));
        }
        let Some(old) = self.docs[self.active].path.clone() else {
            return Err("This file hasn't been saved yet.".into());
        };
        let new = old.with_file_name(name);
        if new == old {
            return Ok(());
        }
        // Only the case changing is the same file, as far as Windows goes.
        if new.exists() && !same_path(&new, &old) {
            return Err(format!("There's already a file called {name} there."));
        }
        std::fs::rename(&old, &new).map_err(|e| format!("Couldn't rename it.\n\n{e}"))?;
        panels::note_recent(&mut self.panels.recent, &new);
        self.panels.recent.retain(|path| !same_path(path, &old) || same_path(path, &new));
        let doc = self.doc();
        doc.path = Some(new.clone());
        doc.disk_stamp = file_stamp(&new);
        doc.resolved_syntax = None;
        Ok(())
    }

    pub(super) fn show_in_explorer(&mut self) {
        let Some(path) = self.docs[self.active].path.clone() else {
            return;
        };
        if let Err(problem) = crate::platform::reveal(&path) {
            self.message(crate::platform::SHOW_IN_FILE_MANAGER, problem);
        }
    }

    pub(super) fn open_with_usual_program(&mut self) {
        let Some(path) = self.docs[self.active].path.clone() else {
            return;
        };
        if let Err(problem) = shell_open(&path) {
            self.message("Open in its usual program", problem);
        }
    }

    // ---------- Snippets ----------

    pub(super) fn insert_snippet(&mut self, index: usize) {
        let Some(snippet) = self.snippets.get(index).cloned() else {
            return;
        };
        if self.no_file() {
            return;
        }
        let doc = &self.docs[self.active];
        if doc.hex_mode {
            self.message("Snippets", "Snippets go into text. Press Ctrl+H to go back to it first.".into());
            return;
        }
        if doc.text.read_only {
            self.tell_read_only();
            return;
        }
        if doc.is_busy() || self.pending.is_some() {
            self.message("Please wait", "The file is still loading or saving. This will be available when it finishes.".into());
            return;
        }
        if !self.one_selection("Snippets") {
            return;
        }
        let doc = &self.docs[self.active];
        let selection = doc.text.format.readable_text(&doc.text.selection_text()).replace("\r\n", "\n");
        let now = chrono::Local::now();
        let (date, time, file) = (now.format("%d/%m/%Y").to_string(), now.format("%H:%M").to_string(), doc.name());
        let (before, after) = expand_snippet(&snippet.body, &Fill { date: &date, time: &time, file: &file, selection: &selection });
        let text = &mut self.doc().text;
        text.begin_undo_group(false);
        if text.has_selection() {
            text.delete_selection();
        }
        text.paste_text(&before, false);
        let (line, character) = text.caret_char_position();
        text.paste_text(&after, false);
        text.move_caret_to_char(line, character);
        text.end_undo_group();
        self.pending_focus = true;
    }

    pub(super) fn snippets_dialog(&mut self, ctx: &Context) -> bool {
        if !matches!(self.dialog, Dialog::Snippets(_)) {
            return false;
        }
        let can_insert = !self.no_file() && !self.docs[self.active].hex_mode;
        let has_selection = can_insert && self.docs[self.active].text.has_selection();
        let Dialog::Snippets(state) = &mut self.dialog else {
            return false;
        };
        let (enter, escape, up, down) = ctx.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape), i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown)));
        let words = state.filter.to_lowercase();
        let shown: Vec<usize> = (0..self.snippets.len()).filter(|at| words.is_empty() || self.snippets[*at].name.to_lowercase().contains(&words) || self.snippets[*at].body.to_lowercase().contains(&words)).collect();
        if state.chosen.is_none_or(|at| !shown.contains(&at)) {
            state.chosen = shown.first().copied();
        }
        let place = state.chosen.and_then(|at| shown.iter().position(|each| *each == at));
        if let Some(place) = place {
            if down {
                state.chosen = shown.get(place + 1).or(shown.last()).copied();
            }
            if up {
                state.chosen = shown.get(place.saturating_sub(1)).copied();
            }
        }
        let (mut insert, mut close, mut add, mut from_selection, mut remove) = (None, escape, false, false, false);
        let mut filter_has_focus = false;
        egui::Window::new("Snippets").collapsible(false).resizable(false).fixed_size(egui::vec2(640.0, 0.0)).anchor(egui::Align2::CENTER_TOP, [0.0, 64.0]).show(ctx, |ui| {
            let field = ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text("Find a snippet").desired_width(f32::INFINITY).margin(egui::vec2(10.0, 7.0)));
            if state.focus {
                field.request_focus();
                state.focus = false;
            }
            filter_has_focus = field.has_focus();
            ui.add_space(6.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(200.0);
                    egui::ScrollArea::vertical().id_salt("snippet-list").min_scrolled_height(260.0).max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                        if shown.is_empty() {
                            ui.label(RichText::new(if self.snippets.is_empty() { "No snippets yet. Press New to make one." } else { "Nothing matches that." }).color(theme::text_faint()));
                        }
                        for at in &shown {
                            let here = state.chosen == Some(*at);
                            let label = ui.add_sized([200.0, 24.0], egui::Button::selectable(here, self.snippets[*at].name.as_str()));
                            if label.clicked() {
                                state.chosen = Some(*at);
                            }
                            if label.double_clicked() && can_insert {
                                insert = Some(*at);
                            }
                        }
                    });
                });
                ui.separator();
                ui.vertical(|ui| {
                    if let Some(snippet) = state.chosen.and_then(|at| self.snippets.get_mut(at)) {
                        ui.horizontal(|ui| {
                            ui.label("Name");
                            ui.add(egui::TextEdit::singleline(&mut snippet.name).desired_width(f32::INFINITY));
                        });
                        ui.add_space(4.0);
                        ui.add(egui::TextEdit::multiline(&mut snippet.body).code_editor().desired_rows(9).desired_width(f32::INFINITY));
                    } else {
                        ui.add_space(40.0);
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new("$CURSOR is where the cursor goes. $SELECTION is what was selected. $DATE, $TIME and $FILE are filled in.").color(theme::text_faint()).size(11.5));
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(can_insert && state.chosen.is_some(), Button::new("Insert")).on_hover_text("Or press Enter, or double-click it").clicked() {
                    insert = state.chosen;
                }
                add = ui.button("New").clicked();
                from_selection = ui.add_enabled(has_selection, Button::new("New from the selection")).clicked();
                remove = ui.add_enabled(state.chosen.is_some(), Button::new("Delete")).clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close |= ui.button("Close").clicked();
                });
            });
        });
        if enter && filter_has_focus && can_insert {
            insert = insert.or(state.chosen);
        }
        if add || from_selection {
            let body = if from_selection {
                let text = &self.docs[self.active].text;
                text.format.readable_text(&text.selection_text()).replace("\r\n", "\n")
            } else {
                String::new()
            };
            self.snippets.push(Snippet { name: format!("Snippet {}", self.snippets.len() + 1), body });
            if let Dialog::Snippets(state) = &mut self.dialog {
                state.filter.clear();
                state.chosen = Some(self.snippets.len() - 1);
            }
        }
        if remove
            && let Dialog::Snippets(state) = &mut self.dialog
            && let Some(at) = state.chosen.take()
        {
            self.snippets.remove(at);
        }
        if let Some(at) = insert {
            self.close_dialog();
            self.insert_snippet(at);
        } else if close {
            self.close_dialog();
        }
        true
    }

    // ---------- Running a command ----------

    pub(super) fn begin_run_tool(&mut self) {
        let line = self.run_history.first().cloned().unwrap_or_default();
        self.dialog = Dialog::RunTool { line, save_first: true, error: String::new(), focus: true };
    }

    /// Runs a command line on the file in front, with what it prints in the Output panel.
    pub(super) fn run_tool(&mut self, line: &str, save_first: bool) -> Result<(), String> {
        if self.run_job.is_some() {
            return Err("A command is still running. Stop it first, in the Output panel.".into());
        }
        let index = self.active;
        let path = if self.no_file() { None } else { self.docs[index].path.clone() };
        let expanded = expand_run_line(line, path.as_deref())?;
        if save_first && path.is_some() && self.docs[index].is_modified() {
            if !self.save(index) {
                return Err("The file couldn't be saved, so nothing was run.".into());
            }
            if self.docs[index].saving.is_some() {
                return Err("The file is being saved. Run it again once that's finished.".into());
            }
        }
        let folder = path.as_deref().and_then(Path::parent).map(Path::to_path_buf).or_else(|| std::env::current_dir().ok());
        let wake = self.waker.clone();
        let job = start_run(&expanded, folder.as_deref(), move || {
            if let Some(ctx) = &wake {
                ctx.request_repaint();
            }
        })?;
        let line = line.trim().to_string();
        self.run_history.retain(|each| *each != line);
        self.run_history.insert(0, line);
        self.run_history.truncate(MOST_RECENT_RUNS);
        self.script_print(format!("> {expanded}"), true);
        self.run_job = Some(job);
        Ok(())
    }

    /// Brings what a running command has printed into the Output panel. True while it's still running.
    pub(super) fn check_run_tool(&mut self) -> bool {
        let Some(job) = &mut self.run_job else {
            return false;
        };
        let fresh: Vec<String> = {
            let lines = job.lines.lock().unwrap();
            let fresh = lines[job.taken..].to_vec();
            job.taken = lines.len();
            fresh
        };
        let finished = job.finished.lock().unwrap().take();
        for line in fresh {
            self.script_print(line, false);
        }
        match finished {
            Some(summary) => {
                self.script_print(summary, false);
                self.run_job = None;
                false
            }
            None => true,
        }
    }

    pub(super) fn stop_tool(&mut self) {
        if let Some(job) = &self.run_job {
            job.stop();
        }
    }

    pub(super) fn tool_running(&self) -> bool {
        self.run_job.is_some()
    }

    // ---------- Spelling ----------

    /// Starts Windows' spell checker the first time it's wanted, and passes it lines and answers.
    pub(super) fn spelling_tick(&mut self) {
        spelling::set_on(self.prefs.spelling && !self.no_file() && !self.docs[self.active].hex_mode);
        if !self.prefs.spelling && !self.spell_asked {
            return;
        }
        if self.speller.is_none() && self.spell_problem.is_none() && !cfg!(test) {
            let wake = self.waker.clone();
            self.speller = spelling::Speller::start(move || {
                if let Some(ctx) = &wake {
                    ctx.request_repaint();
                }
            });
        }
        // Lines stay waiting until there's a checker to ask.
        let Some(speller) = &self.speller else {
            return;
        };
        let wanted = spelling::take_wanted();
        if !wanted.is_empty() {
            speller.ask(Ask::Check(wanted));
        }
        let mut found = None;
        let mut unavailable = None;
        while let Ok(told) = speller.told.try_recv() {
            match told {
                Told::Checked(results) => spelling::remember(results),
                Told::Found(result) => found = Some(result),
                Told::Unavailable(problem) => unavailable = Some(problem),
            }
        }
        if let Some(problem) = unavailable {
            self.spell_problem = Some(problem.clone());
            self.prefs.spelling = false;
            self.spell_asked = false;
            self.message("Spelling", problem);
        }
        if self.spell_problem.is_some() {
            self.speller = None;
        }
        if let Some(result) = found.filter(|_| self.spell_asked) {
            self.spell_asked = false;
            match result {
                Some(found) => {
                    self.doc().text.select_chars((found.line, found.start), (found.line, found.end));
                    self.dialog = Dialog::Spelling { found, chosen: 0 };
                }
                None => self.message("Spelling", "No spelling mistakes were found.".into()),
            }
        }
    }

    /// Looks for the next mistake after the cursor, going round to the start.
    pub(super) fn next_mistake(&mut self) {
        if let Some(problem) = &self.spell_problem {
            let problem = problem.clone();
            self.message("Spelling", problem);
            return;
        }
        let text = &self.docs[self.active].text;
        let count = text.line_count();
        let (here, column) = text.caret_char_position();
        let from = if text.has_selection() { text.caret_char_position().1.max(text.anchor_char_position().1) } else { column };
        let mut lines = Vec::new();
        let mut first = true;
        for line in (here..count).chain(0..here).take(spelling::MOST_LINES_SEARCHED) {
            let whole = text.format.readable_text(&text.line(line));
            // The rest of the cursor's line, with what's before the cursor blanked so places still count right.
            let checked: String = if first { whole.chars().enumerate().map(|(at, c)| if at < from { ' ' } else { c }).collect() } else { whole };
            first = false;
            lines.push((line, checked));
        }
        self.spell_asked = true;
        self.spelling_tick();
        if let Some(speller) = &self.speller {
            speller.ask(Ask::FindNext(lines));
        }
    }

    pub(super) fn spelling_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::Spelling { found, chosen } = &mut self.dialog else {
            return false;
        };
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let (mut change, mut ignore, mut add, mut skip, mut close) = (false, false, false, false, escape);
        egui::Window::new("Spelling").collapsible(false).resizable(false).fixed_size(egui::vec2(380.0, 0.0)).anchor(egui::Align2::RIGHT_TOP, [-40.0, 90.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Not in the dictionary:");
                ui.label(RichText::new(found.word.as_str()).strong().color(theme::rainbow_mistake()));
            });
            ui.add_space(6.0);
            if found.suggestions.is_empty() {
                ui.label(RichText::new("No suggestions.").color(theme::text_faint()));
            }
            for (index, word) in found.suggestions.iter().enumerate() {
                let pick = ui.add_sized([360.0, 24.0], egui::Button::selectable(*chosen == index, word.as_str()));
                if pick.clicked() {
                    *chosen = index;
                }
                if pick.double_clicked() {
                    change = true;
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                change |= ui.add_enabled(!found.suggestions.is_empty(), Button::new("Change")).clicked();
                skip = ui.button("Skip").on_hover_text("Leave this one, and go on to the next").clicked();
                ignore = ui.button("Ignore everywhere").on_hover_text("Until Limoniter is closed").clicked();
                add = ui.button("Add to dictionary").on_hover_text(if cfg!(windows) { "Windows' own dictionary, so other programs know it too" } else { "Your own dictionary, kept by hunspell or aspell" }).clicked();
            });
            ui.add_space(4.0);
            close |= ui.button("Close").clicked();
        });
        let answer = if close {
            None
        } else if change && let Some(word) = found.suggestions.get(*chosen) {
            Some(SpellAnswer::Change(word.clone()))
        } else if ignore {
            Some(SpellAnswer::Ignore)
        } else if add {
            Some(SpellAnswer::Add)
        } else if skip {
            Some(SpellAnswer::Skip)
        } else {
            return true;
        };
        match answer {
            Some(answer) => self.spelling_answer(answer),
            None => self.close_dialog(),
        }
        true
    }

    /// What the person chose in the Spelling box. Each goes on to the next mistake.
    pub(super) fn spelling_answer(&mut self, answer: SpellAnswer) {
        let Dialog::Spelling { found, .. } = &self.dialog else {
            return;
        };
        let found = found.clone();
        if let SpellAnswer::Change(word) = &answer {
            let text = &mut self.doc().text;
            // Only if the word is still where it was found.
            let line = text.format.readable_text(&text.line(found.line));
            let there: String = line.chars().skip(found.start).take(found.end - found.start).collect();
            if there == found.word && !text.read_only {
                text.select_chars((found.line, found.start), (found.line, found.end));
                text.type_text(word);
            }
        }
        if matches!(answer, SpellAnswer::Ignore | SpellAnswer::Add) {
            if let Some(speller) = &self.speller {
                speller.ask(if answer == SpellAnswer::Add { Ask::Add(found.word.clone()) } else { Ask::Ignore(found.word.clone()) });
            }
            spelling::forget_all();
        }
        self.close_dialog();
        self.next_mistake();
    }

    /// For pictures of these, partway through.
    pub(super) fn file_tools_screenshot_step(&mut self, step: &str) {
        match step {
            "snippets" => {
                self.dialog = Dialog::Snippets(SnippetBox { focus: true, ..SnippetBox::default() });
            }
            "run-tool" => {
                self.run_history = vec!["python \"%FILE%\"".into(), "git diff -- \"%NAME%\"".into(), "dir /b".into()];
                self.begin_run_tool();
            }
            "tool-output" => {
                let _ = self.run_tool("echo Lines in %NAME%: & find /c /v \"\" \"%FILE%\" & dir /b", false);
            }
            "spelling" | "spell-box" => {
                self.prefs.spelling = true;
                let text = &mut self.doc().text;
                text.move_caret(1, 0, false);
                text.type_text("Thsi line has a mistaek or two in it, to show how they look.");
                text.move_caret(0, 0, false);
                if step == "spell-box" {
                    self.next_mistake();
                }
            }
            "rename" => self.begin_rename(),
            "split-box" => self.begin_split(),
            "cycle" => {
                for (name, text) in [("Edit2", "A second file"), ("Edit3", "A third file")] {
                    self.new_document();
                    self.doc().untitled = name.into();
                    self.doc().text.set_content(vec![text.into()]);
                    self.used_order.insert(0, self.docs[self.active].id);
                }
                self.cycle_used(true);
                self.cycle_used(true);
                self.hold_cycle = true;
            }
            _ => {}
        }
    }

    // ---------- The boxes ----------

    pub(super) fn file_tool_dialogs(&mut self, ctx: &Context) -> bool {
        if self.snippets_dialog(ctx) || self.spelling_dialog(ctx) {
            return true;
        }
        let title = match &self.dialog {
            Dialog::Rename { .. } => "Rename",
            Dialog::SplitFile { .. } => "Split into parts",
            Dialog::RunTool { .. } => "Run a command",
            _ => return false,
        };
        let (enter, escape) = ctx.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
        let mut close = escape;
        let mut rename = None;
        let mut split = None;
        let mut run = None;
        let history = self.run_history.clone();
        egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| match &mut self.dialog {
            Dialog::Rename { name, error, focus } => {
                ui.label("New name, in the same folder:");
                let field = ui.add(egui::TextEdit::singleline(name).desired_width(340.0));
                if *focus {
                    field.request_focus();
                    // The name is chosen without its extension, ready to type over.
                    let stem = Path::new(name.as_str()).file_stem().map_or(0, |stem| stem.to_string_lossy().chars().count());
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field.id) {
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(stem))));
                        state.store(ui.ctx(), field.id);
                    }
                    *focus = false;
                }
                if !error.is_empty() {
                    ui.label(RichText::new(error.as_str()).color(theme::rainbow_mistake()));
                }
                let (ok, cancel) = ok_cancel(ui);
                close |= cancel;
                if ok || enter {
                    rename = Some(name.clone());
                }
            }
            Dialog::SplitFile { every, by_text, text, error } => {
                ui.label("Writes the file as several, beside it, named name.part-001 and so on. The file itself isn't changed.");
                ui.add_space(6.0);
                ui.radio_value(by_text, false, "A new part every so many lines");
                ui.add_enabled_ui(!*by_text, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(22.0);
                        ui.add(egui::TextEdit::singleline(every).desired_width(90.0));
                        ui.label("lines in each part");
                    });
                });
                ui.radio_value(by_text, true, "A new part at each line containing");
                ui.add_enabled_ui(*by_text, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(22.0);
                        ui.add(egui::TextEdit::singleline(text).desired_width(240.0).hint_text("such as a heading"));
                    });
                });
                if !error.is_empty() {
                    ui.label(RichText::new(error.as_str()).color(theme::rainbow_mistake()));
                }
                let (ok, cancel) = ok_cancel(ui);
                close |= cancel;
                if ok || enter {
                    split = Some(if *by_text {
                        if text.is_empty() { Err("Type what the lines that start each part contain.".to_string()) } else { Ok(SplitBy::Containing(text.clone())) }
                    } else {
                        match every.trim().parse::<usize>() {
                            Ok(n) if n > 0 => Ok(SplitBy::Lines(n)),
                            _ => Err("Type how many lines go in each part, as a whole number.".to_string()),
                        }
                    });
                }
            }
            Dialog::RunTool { line, save_first, error, focus } => {
                ui.label(format!("A command, run as in {}, in this file's folder:", crate::platform::SHELL_NAME));
                let field = ui.add(egui::TextEdit::singleline(line).desired_width(460.0).font(egui::TextStyle::Monospace).hint_text("python \"%FILE%\""));
                if *focus {
                    field.request_focus();
                    *focus = false;
                }
                ui.label(RichText::new("%FILE% is this file's full path, %NAME% its name, %STEM% its name without the extension, and %DIR% its folder. Put quotes round them if they might have spaces.").color(theme::text_faint()).size(11.5));
                ui.checkbox(save_first, "Save the file first");
                if !history.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Run before").color(theme::text_dim()));
                    egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                        for each in &history {
                            if ui.add(egui::Button::selectable(each == line, RichText::new(each).monospace())).clicked() {
                                line.clone_from(each);
                            }
                        }
                    });
                }
                if !error.is_empty() {
                    ui.label(RichText::new(error.as_str()).color(theme::rainbow_mistake()));
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Run").clicked() {
                        run = Some((line.clone(), *save_first));
                    }
                    close |= ui.button("Cancel").clicked();
                });
                if enter {
                    run = Some((line.clone(), *save_first));
                }
            }
            _ => {}
        });
        if let Some(name) = rename {
            match self.rename_to(&name) {
                Ok(()) => close = true,
                Err(problem) => {
                    if let Dialog::Rename { error, .. } = &mut self.dialog {
                        *error = problem;
                    }
                }
            }
        }
        if let Some(choice) = split {
            match choice {
                Ok(by) => {
                    self.start_split(by);
                    close = true;
                }
                Err(problem) => {
                    if let Dialog::SplitFile { error, .. } = &mut self.dialog {
                        *error = problem;
                    }
                }
            }
        }
        if let Some((line, save_first)) = run {
            match self.run_tool(&line, save_first) {
                Ok(()) => close = !matches!(self.dialog, Dialog::Message { .. }) || close,
                Err(problem) => {
                    if let Dialog::RunTool { error, .. } = &mut self.dialog {
                        *error = problem;
                    }
                }
            }
        }
        if close && matches!(self.dialog, Dialog::Rename { .. } | Dialog::SplitFile { .. } | Dialog::RunTool { .. }) {
            self.close_dialog();
        }
        true
    }
}
