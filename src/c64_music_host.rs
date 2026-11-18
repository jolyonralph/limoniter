//! The window's side of the C64 tools (Tools, Commodore 64, SID music and packers): a SID tune's details, its
//! code opened as a program, the calls that play it put into 6502 source, and the tune played in vsid or sent to an
//! Ultimate; packing the program in front, or the one chosen in a disk image's browser, with Exomizer, TSCrunch,
//! pucrunch or ZX0; a plain cartridge made from a binary; and, when the setting is on, packing what F9 builds.
//! Every tool runs in the background, and a missing tool only ever means a note.

use super::{Dialog, Document, Limoniter, theme};
use crate::amiga_tools::{Outcome, group};
use crate::assemblers::Cpu;
use crate::c64_tools::{self as tools, PackOptions, Prefs as ToolPrefs, Start, Tool};
use crate::deploy::Place;
use eframe::egui::{self, Button, Context, Key, RichText};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Music,
    Pack,
    Cartridge,
    Tools,
}

/// The file the box works on: on the disk, or taken out of an image or not saved yet.
struct Source {
    name: String,
    path: Option<PathBuf>,
    bytes: Arc<Vec<u8>>,
    /// Where what's made goes.
    folder: Option<PathBuf>,
    tune: Option<crate::sid::Tune>,
    /// Whether it's the document in front itself, which File, Send to device can send.
    in_front: bool,
}

/// A tool running in the background.
pub(super) struct Job {
    what: String,
    stop: Arc<AtomicBool>,
    rx: Receiver<Result<Outcome, String>>,
}

pub(super) struct C64ToolsBox {
    page: Page,
    prefs: ToolPrefs,
    found: Vec<(Tool, Option<PathBuf>)>,
    source: Option<Source>,
    song: u16,
    output_name: String,
    job: Option<Job>,
    result: Option<Result<Outcome, String>>,
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
    let folder = std::env::temp_dir().join(format!("limoniter-c64-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).map_err(|e| crate::i18n::tf!("A folder to work in couldn't be made: {e}"))?;
    let safe = name.rsplit(['/', '\\', ':', '\u{203A}']).next().unwrap_or(name).trim();
    let path = folder.join(if safe.is_empty() { "file" } else { safe });
    std::fs::write(&path, bytes).map_err(|e| crate::i18n::tf!("{} couldn't be written: {e}", path.display()))?;
    Ok((folder, path))
}

/// The lines for the Output panel about what a packer did.
fn report(what: &str, result: &Result<Outcome, String>) -> Vec<String> {
    let mut lines = vec![what.to_string()];
    match result {
        Ok(outcome) => {
            lines.extend(outcome.printed.iter().cloned());
            if outcome.ok {
                let sizes = outcome.sizes();
                if !sizes.is_empty() {
                    lines.push(crate::i18n::tf!("Packed in {:.1} s: {sizes}.", outcome.seconds));
                }
                lines.extend(outcome.made.iter().map(|(path, size)| crate::i18n::tf!("Wrote {} ({} bytes).", path.display(), group(*size))));
            } else {
                lines.push(crate::i18n::tr("It didn't finish: see what it printed above.").into());
            }
        }
        Err(problem) => lines.push(problem.clone()),
    }
    lines
}

/// The file's own name, without the image or archive it came out of.
fn own_name(name: &str) -> &str {
    name.rsplit(['/', '\\', '\u{203A}']).next().unwrap_or(name).trim()
}

impl Limoniter {
    /// The file the box is for: the file chosen in a browser tab, or the document in front.
    fn c64_tools_source(&self) -> Option<Source> {
        if self.no_file() {
            return None;
        }
        let doc = &self.docs[self.active];
        let (name, path, bytes, folder, in_front) = if let Some(browser) = &doc.browser {
            let (name, bytes) = browser.selected_file()?.ok()?;
            (name, None, bytes, doc.path.as_deref().and_then(Path::parent).map(Path::to_path_buf), false)
        } else if let Some(path) = doc.path.clone().filter(|_| !doc.is_modified() && !doc.text.is_disk_backed()) {
            let bytes = std::fs::read(&path).ok()?;
            (doc.name(), Some(path.clone()), bytes, path.parent().map(Path::to_path_buf), true)
        } else {
            let bytes = if doc.hex_mode { doc.hex.bytes() } else { doc.text.encode(&doc.text.format, crate::codec::Lossy::Omit).ok()? };
            (doc.name(), None, bytes, doc.path.as_deref().and_then(Path::parent).map(Path::to_path_buf), true)
        };
        let tune = crate::sid::looks_like(&bytes).then(|| crate::sid::read(&bytes).ok()).flatten();
        Some(Source { name, path, bytes: Arc::new(bytes), folder, tune, in_front })
    }

    /// Opens the box (Tools, Commodore 64), on the page that suits the file in front.
    pub(super) fn open_c64_tools(&mut self) {
        let prefs = ToolPrefs::load();
        let source = self.c64_tools_source();
        let page = match &source {
            Some(s) if s.tune.is_some() => Page::Music,
            Some(s) => {
                let name = own_name(&s.name).to_ascii_lowercase();
                let at_8000 = name.ends_with(".prg") && s.bytes.len() <= 16384 + 2 && s.bytes.starts_with(&[0x00, 0x80]);
                let rom = !name.ends_with(".prg") && !name.ends_with(".crt") && matches!(s.bytes.len(), 8192 | 16384);
                if at_8000 || rom { Page::Cartridge } else { Page::Pack }
            }
            None => Page::Tools,
        };
        let output_name = source.as_ref().map(|s| tools::packed_name(own_name(&s.name), prefs.packer, &prefs.options)).unwrap_or_default();
        let song = source.as_ref().and_then(|s| s.tune.as_ref()).map_or(1, |t| t.start());
        let found = find_all(&prefs);
        self.dialog = Dialog::C64Tools(Box::new(C64ToolsBox { page, prefs, found, source, song, output_name, job: None, result: None, status: String::new() }));
    }

    /// The 6502 source a snippet goes into: the document in front if it's 6502 source, else the first open one.
    fn m6502_target(&self) -> Option<usize> {
        if self.doc_cpu(self.active) == Some(Cpu::M6502) && !self.docs[self.active].text.read_only {
            return Some(self.active);
        }
        (0..self.docs.len()).find(|&i| self.doc_cpu(i) == Some(Cpu::M6502) && !self.docs[i].text.read_only)
    }

    pub(super) fn c64_tools_dialog(&mut self, ctx: &Context) -> bool {
        let target = self.m6502_target().map(|i| self.docs[i].name());
        let ultimates: Vec<(usize, String)> = self.sends.devices.iter().enumerate().filter(|(_, d)| matches!(d.place, Place::Ultimate { .. })).map(|(i, d)| (i, d.name.clone())).collect();
        let Dialog::C64Tools(state) = &mut self.dialog else {
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
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut action: Option<Act> = None;
        let busy = state.job.is_some();
        egui::Window::new(crate::i18n::tr("Commodore 64: SID music and packers")).collapsible(false).resizable(true).default_width(600.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(640.0);
            ui.horizontal(|ui| {
                for (page, label) in [(Page::Music, "SID music"), (Page::Pack, "Pack a program"), (Page::Cartridge, "Cartridge"), (Page::Tools, "Tools")] {
                    if ui.selectable_label(state.page == page, crate::i18n::tr(label)).clicked() {
                        state.page = page;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| match state.page {
                Page::Music => music_page(ui, state, target.as_deref(), &ultimates, &mut action),
                Page::Pack => pack_page(ui, state, &mut action),
                Page::Cartridge => cartridge_page(ui, state, &mut action),
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
            self.c64_tools_act(ctx, act);
        }
        if let Dialog::C64Tools(state) = &self.dialog
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
    fn c64_tools_act(&mut self, ctx: &Context, act: Act) {
        let target = self.m6502_target().map(|i| (i, self.docs[i].name()));
        let say = |app: &mut Limoniter, text: String| {
            if let Dialog::C64Tools(state) = &mut app.dialog {
                state.status = text;
            }
        };
        match act {
            Act::Pack => self.c64_pack(ctx),
            Act::Insert(text) => match target {
                Some((index, name)) => {
                    self.docs[index].text.paste_text(&text, false);
                    self.active = index;
                    say(self, crate::i18n::tf!("The replay calls were put into {name}, at the cursor."));
                }
                None => ctx.copy_text(text),
            },
            Act::Copy(text) => ctx.copy_text(text),
            Act::OpenProgram => {
                let Dialog::C64Tools(state) = &self.dialog else {
                    return;
                };
                let Some((name, program)) = state.source.as_ref().and_then(|s| s.tune.as_ref().map(|t| (s.name.clone(), t.program()))) else {
                    return;
                };
                let stem = own_name(&name).rsplit_once('.').map_or(own_name(&name), |(s, _)| s).to_string();
                let doc = Document::from_image_file(format!("{} \u{203A} {stem}.prg", own_name(&name)), program, None);
                self.docs.push(doc);
                self.active = self.docs.len() - 1;
                self.dialog = Dialog::None;
                self.message("The tune's code", crate::i18n::tf!("{stem}.prg is the tune's code and data at its load address, as a C64 program. Tools, Disassemble\u{2026} turns it into 6502 source, starting from its init and play routines; File, Save as keeps it."));
            }
            Act::Vsid => {
                let Dialog::C64Tools(state) = &self.dialog else {
                    return;
                };
                let Some(vsid) = found(&state.found, Tool::Vsid) else {
                    return;
                };
                let song = state.song;
                let Some(source) = &state.source else {
                    return;
                };
                let path = match &source.path {
                    Some(path) => Ok(path.clone()),
                    None => scratch_copy(own_name(&source.name), &source.bytes).map(|(_, path)| path),
                };
                let said = path.and_then(|path| tools::play_in_vsid(&vsid, &path, song)).map_or_else(|problem| problem, |()| crate::i18n::tf!("vsid is playing song {song}, in its own window."));
                say(self, said);
            }
            Act::Send(device) => {
                self.dialog = Dialog::None;
                self.open_send_box(device);
            }
            Act::Cartridge => {
                let Dialog::C64Tools(state) = &mut self.dialog else {
                    return;
                };
                let Some(source) = &state.source else {
                    return;
                };
                let stem = own_name(&source.name).rsplit_once('.').map_or(own_name(&source.name), |(s, _)| s).to_string();
                let made = crate::media::crt::cartridge_binary(&source.bytes).and_then(|binary| crate::media::crt::normal_cartridge(&binary, &stem)).and_then(|crt| {
                    let folder = source.folder.clone().ok_or(crate::i18n::tr("Choose a folder for the cartridge first."))?;
                    let path = folder.join(format!("{stem}.crt"));
                    std::fs::write(&path, &crt).map_err(|e| crate::i18n::tf!("{} couldn't be written: {e}", path.display()))?;
                    Ok(Outcome { ok: true, made: vec![(path, crt.len() as u64)], ..Outcome::default() })
                });
                state.result = Some(made);
            }
            Act::Choose(tool) => {
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tf!("Where is {}?", tool.name())).pick_file()
                    && let Dialog::C64Tools(state) = &mut self.dialog
                {
                    state.prefs.set_path(tool, Some(path));
                    state.found = find_all(&state.prefs);
                }
            }
            Act::Forget(tool) => {
                if let Dialog::C64Tools(state) = &mut self.dialog {
                    state.prefs.set_path(tool, None);
                    state.found = find_all(&state.prefs);
                }
            }
            Act::Folder => {
                if let Some(folder) = rfd::FileDialog::new().set_title(crate::i18n::tr("Where to put what's made")).pick_folder()
                    && let Dialog::C64Tools(state) = &mut self.dialog
                    && let Some(source) = &mut state.source
                {
                    source.folder = Some(folder);
                }
            }
        }
    }

    fn c64_pack(&mut self, ctx: &Context) {
        let Dialog::C64Tools(state) = &mut self.dialog else {
            return;
        };
        let Some(source) = &state.source else {
            return;
        };
        let tool = state.prefs.packer;
        let Some(program) = found(&state.found, tool) else {
            state.result = Some(Err(crate::i18n::tf!("{} wasn't found. {}", tool.name(), tool.how_to_get())));
            return;
        };
        let Some(folder) = source.folder.clone() else {
            state.result = Some(Err(crate::i18n::tr("Choose a folder for the packed file first.").into()));
            return;
        };
        let output = folder.join(state.output_name.trim());
        if state.output_name.trim().is_empty() || source.path.as_deref() == Some(output.as_path()) {
            state.result = Some(Err(crate::i18n::tr("Give the packed file a name of its own.").into()));
            return;
        }
        let options = state.prefs.options.clone();
        let (path, bytes, name) = (source.path.clone(), source.bytes.clone(), own_name(&source.name).to_string());
        let what = crate::i18n::tf!("Packing {name} with {}", tool.name());
        state.result = None;
        state.job = Some(start(Some(ctx), what, move |stop| {
            let (scratch, input) = match path {
                Some(path) => (None, path),
                None => {
                    let (folder, path) = scratch_copy(&name, &bytes)?;
                    (Some(folder), path)
                }
            };
            let result = tools::pack(tool, &program, &options, &input, &output, stop);
            if let Some(folder) = scratch {
                let _ = std::fs::remove_dir_all(folder);
            }
            result
        }));
    }

    /// After F9 builds a C64 program, packs a copy of it beside the source when the setting asks, in the
    /// background. Without the setting, or for anything that isn't a C64 program, nothing happens; without the
    /// packer, the Output panel says so.
    pub(super) fn c64_pack_after_build(&mut self, doc_id: u64, name: &str, bytes: &[u8]) {
        if !tools::is_program(name, bytes) {
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
            self.script_print(crate::i18n::tf!("Packing after building is on, but {} wasn't found, so the program wasn't packed (Tools, Commodore 64, SID music and packers).", tool.name()), false);
            return;
        };
        // A program packed after building always runs: it's the one F9 would run.
        let options = PackOptions { runnable: true, ..prefs.options.clone() };
        let output = folder.join(tools::packed_name(name, tool, &options));
        let (name, bytes) = (name.to_string(), bytes.to_vec());
        let what = crate::i18n::tf!("Packing {name} with {}", tool.name());
        let job = start(None, what.clone(), move |stop| {
            let (scratch, input) = scratch_copy(&name, &bytes)?;
            let result = tools::pack(tool, &program, &options, &input, &output, stop);
            let _ = std::fs::remove_dir_all(scratch);
            result
        });
        self.script_print(crate::i18n::tf!("{what}, in the background\u{2026}"), false);
        self.c64_jobs.push(job);
    }

    /// Reports packing after building when it's done.
    pub(super) fn c64_jobs_tick(&mut self, ctx: &Context) {
        if self.c64_jobs.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        let mut done = Vec::new();
        self.c64_jobs.retain(|job| match job.rx.try_recv() {
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
    Pack,
    Insert(String),
    Copy(String),
    OpenProgram,
    Vsid,
    Send(usize),
    Cartridge,
    Choose(Tool),
    Forget(Tool),
    Folder,
}

fn note(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(theme::text_dim()).size(12.0));
}

fn file_line(ui: &mut egui::Ui, source: &Source, action: &mut Option<Act>) {
    ui.horizontal_wrapped(|ui| {
        ui.strong(own_name(&source.name));
        ui.label(crate::i18n::tf!("{} bytes", group(source.bytes.len() as u64)));
    });
    ui.horizontal_wrapped(|ui| {
        match &source.folder {
            Some(folder) => note(ui, crate::i18n::tf!("What's made goes in {}", folder.display())),
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
            note(ui, "Point Limoniter at it on the Tools page.");
            false
        }
    }
}

fn music_page(ui: &mut egui::Ui, state: &mut C64ToolsBox, target: Option<&str>, ultimates: &[(usize, String)], action: &mut Option<Act>) {
    let Some(source) = &state.source else {
        ui.label(crate::i18n::tr("Open a SID tune (.sid), or choose one in a disk image's browser, then come back here."));
        return;
    };
    file_line(ui, source, action);
    let Some(tune) = &source.tune else {
        ui.colored_label(theme::rainbow_mistake(), crate::i18n::tr("This isn't a SID tune: it doesn't start with PSID or RSID."));
        return;
    };
    let by = if tune.author.is_empty() { String::new() } else { crate::i18n::tf!(" by {}", tune.author) };
    let name = if tune.name.is_empty() { crate::i18n::tr("Untitled").to_string() } else { tune.name.clone() };
    let released = if tune.released.is_empty() { String::new() } else { format!(" ({})", tune.released) };
    let (load, init) = (tune.load, tune.init_address());
    let play = if tune.play == 0 { crate::i18n::tr("its own").to_string() } else { format!("${:04X}", tune.play) };
    let songs = tune.songs;
    note(ui, if songs == 1 { crate::i18n::tf!("{name}{by}{released}. Loads at ${load:04X}, init ${init:04X}, play {play}. 1 song.") } else { crate::i18n::tf!("{name}{by}{released}. Loads at ${load:04X}, init ${init:04X}, play {play}. {songs} songs.") });
    note(ui, "File properties shows all its header says.");
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Song:"));
        ui.add(egui::DragValue::new(&mut state.song).range(1..=tune.songs.max(1)));
        note(ui, crate::i18n::tf!("(it starts with {})", tune.start()));
    });
    ui.horizontal_wrapped(|ui| {
        if ui.button(crate::i18n::tr("Open its code as a program")).on_hover_text(crate::i18n::tr("The tune's bytes at its load address, as a .prg, to disassemble")).clicked() {
            *action = Some(Act::OpenProgram);
        }
        let vsid = found(&state.found, Tool::Vsid);
        if ui.add_enabled(vsid.is_some(), Button::new(crate::i18n::tr("Play in VICE (vsid)"))).on_disabled_hover_text(Tool::Vsid.how_to_get()).clicked() {
            *action = Some(Act::Vsid);
        }
        if source.in_front {
            for (index, name) in ultimates {
                if ui.button(crate::i18n::tf!("Send to {name}")).on_hover_text(crate::i18n::tr("The Ultimate plays it as soon as it arrives")).clicked() {
                    *action = Some(Act::Send(*index));
                }
            }
        }
    });
    if ultimates.is_empty() {
        note(ui, "To play it on an Ultimate 64 or 1541 Ultimate, add one under File, Send to device, Manage devices.");
    }
    ui.add_space(6.0);
    ui.strong("Replay calls");
    note(ui, "Written by Limoniter from the tune's header, for any assembler that takes ; comments: init with the song, then play from a raster interrupt once a frame (or as the tune says).");
    let snippet = crate::sid::replay_snippet(tune, own_name(&source.name), state.song);
    let mut shown = snippet.as_str();
    ui.add(egui::TextEdit::multiline(&mut shown).code_editor().desired_rows(12).desired_width(f32::INFINITY));
    ui.horizontal(|ui| {
        match target {
            Some(name) => {
                if ui.button(crate::i18n::tf!("Insert into {name}")).on_hover_text(crate::i18n::tr("At the cursor, as one change to undo")).clicked() {
                    *action = Some(Act::Insert(snippet.clone()));
                }
            }
            None => note(ui, "Open 6502 source to insert them into it."),
        }
        if ui.button(crate::i18n::tr("Copy")).clicked() {
            *action = Some(Act::Copy(snippet.clone()));
        }
    });
}

fn pack_page(ui: &mut egui::Ui, state: &mut C64ToolsBox, action: &mut Option<Act>) {
    let Some(source) = &state.source else {
        ui.label(crate::i18n::tr("Open a C64 program (.prg, it can be in hex) or data, or choose one in a disk image's browser, then come back here."));
        return;
    };
    file_line(ui, source, action);
    let name = own_name(&source.name).to_string();
    let program = tools::is_program(&name, &source.bytes);
    if program {
        let load = u16::from_le_bytes([source.bytes[0], source.bytes[1]]);
        let sys = tools::sys_address(&source.bytes).map_or(String::new(), |s| crate::i18n::tf!(", its BASIC line calls SYS {s} (${s:04X})"));
        note(ui, crate::i18n::tf!("A C64 program loading at ${load:04X}{sys}."));
    } else {
        note(ui, "Not a .prg, so it's packed as data.");
    }
    ui.add_space(4.0);
    let before = (state.prefs.packer, state.prefs.options.clone());
    ui.horizontal_wrapped(|ui| {
        for tool in Tool::PACKERS {
            ui.radio_value(&mut state.prefs.packer, tool, crate::i18n::tr(tool.name()));
        }
    });
    let tool = state.prefs.packer;
    let ready = tool_status(ui, &state.found, tool);
    ui.add_space(4.0);
    let o = &mut state.prefs.options;
    if tool.makes_runnable() && program {
        ui.radio_value(&mut o.runnable, true, crate::i18n::tr("A program that unpacks and starts itself"));
        ui.add_enabled_ui(o.runnable, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                let sys_hint = match tool {
                    Tool::Exomizer => "exomizer sfx sys",
                    Tool::Pucrunch => "pucrunch reads the SYS line itself",
                    _ => "Limoniter reads the SYS line and gives TSCrunch its address",
                };
                ui.radio_value(&mut o.start, Start::Sys, crate::i18n::tr("At its SYS")).on_hover_text(sys_hint);
                if tool != Tool::TsCrunch {
                    ui.radio_value(&mut o.start, Start::Basic, crate::i18n::tr("As BASIC (RUN)")).on_hover_text(if tool == Tool::Exomizer { crate::i18n::tr("exomizer sfx basic") } else { crate::i18n::tr("pucrunch -fbasic") });
                } else if o.start == Start::Basic {
                    o.start = Start::Sys;
                }
                ui.radio_value(&mut o.start, Start::Address, crate::i18n::tr("At"));
                ui.add(egui::TextEdit::singleline(&mut o.address).desired_width(60.0).hint_text(crate::i18n::tr("$0810")));
            });
        });
        match tool {
            Tool::TsCrunch => note(ui, "TSCrunch's unpacked program can't return to BASIC with rts, and text it prints through the KERNAL may not show: it suits a demo that takes the machine over."),
            Tool::Pucrunch => note(ui, "pucrunch may fail to start a very small BASIC program (a few dozen bytes) once unpacked."),
            _ => {}
        }
        ui.radio_value(&mut o.runnable, false, crate::i18n::tr("Data, for your own code to unpack"));
    } else {
        note(ui, if tool.makes_runnable() { "It isn't a program, so it's packed as data for your own code to unpack." } else { "ZX0 packs data, for your own code to unpack." });
    }
    let data = !(o.runnable && tool.makes_runnable() && program);
    if data {
        ui.horizontal_wrapped(|ui| match tool {
            Tool::Exomizer => {
                ui.add_enabled(program, egui::Checkbox::new(&mut o.exo_mem, "As a program to unpack in place (mem), not raw")).on_hover_text(crate::i18n::tr("exomizer mem keeps the load address; raw packs the bytes alone"));
                ui.checkbox(&mut o.backwards, crate::i18n::tr("Unpacked backwards"));
            }
            Tool::TsCrunch => {
                ui.checkbox(&mut o.inplace, crate::i18n::tr("In place (-i)"));
            }
            Tool::Salvador | Tool::Zx0 => {
                ui.checkbox(&mut o.zx0_classic, crate::i18n::tr("ZX0 version 1 (classic)")).on_hover_text(crate::i18n::tr("For depackers older than ZX0 version 2"));
                ui.checkbox(&mut o.backwards, crate::i18n::tr("Backwards (-b)"));
            }
            _ => {}
        });
        note(ui, crate::i18n::tf!("To unpack it: {}", tools::decruncher(tool, o)));
    }
    if (state.prefs.packer, state.prefs.options.clone()) != before {
        state.output_name = tools::packed_name(&name, state.prefs.packer, &state.prefs.options);
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Save as:"));
        ui.text_edit_singleline(&mut state.output_name);
    });
    let busy = state.job.is_some();
    if ui.add_enabled(ready && !busy, Button::new(crate::i18n::tf!("Pack with {}", tool.name()))).clicked() {
        *action = Some(Act::Pack);
    }
    note(ui, "A packed program can be tried at once: open it, then F9 runs it in VICE or on an Ultimate.");
}

fn cartridge_page(ui: &mut egui::Ui, state: &mut C64ToolsBox, action: &mut Option<Act>) {
    note(ui, "A normal C64 cartridge (type 0) from an 8K or 16K binary, or from a program that loads at $8000: 8K is ROML at $8000 with EXROM active; 16K adds ROMH at $A000, with GAME active too. The binary starts with the cartridge's cold and warm start addresses and CBM80.");
    let Some(source) = &state.source else {
        ui.label(crate::i18n::tr("Open the binary, then come back here."));
        return;
    };
    file_line(ui, source, action);
    match crate::media::crt::cartridge_binary(&source.bytes) {
        Ok(binary) => {
            let signature = binary.get(4..9) == Some(&[0xC3, 0xC2, 0xCD, 0x38, 0x30]);
            note(ui, format!("{}K cartridge{}", binary.len() / 1024, if signature { ", with its CBM80 signature." } else { ". It has no CBM80 signature at $8004, so the C64 won't start it by itself." }));
            if ui.button(crate::i18n::tr("Make the .crt")).clicked() {
                *action = Some(Act::Cartridge);
            }
        }
        Err(problem) => {
            ui.colored_label(theme::rainbow_mistake(), problem);
        }
    }
}

fn tools_page(ui: &mut egui::Ui, state: &mut C64ToolsBox, action: &mut Option<Act>) {
    note(ui, "Limoniter runs these where they're installed, or where you point it. It never fetches one.");
    ui.add_space(4.0);
    for tool in Tool::ALL {
        ui.horizontal_wrapped(|ui| {
            ui.strong(tool.name());
            ui.hyperlink_to("website", tool.website());
        });
        note(ui, tool.licence());
        match found(&state.found, tool) {
            Some(path) => note(ui, format!("Found: {}", path.display())),
            None => note(ui, crate::i18n::tf!("Not found. {}", tool.how_to_get())),
        }
        ui.horizontal(|ui| {
            if ui.small_button(crate::i18n::tr("Choose\u{2026}")).on_hover_text(crate::i18n::tr("Point Limoniter at the program")).clicked() {
                *action = Some(Act::Choose(tool));
            }
            if state.prefs.path(tool).is_some() && ui.small_button(crate::i18n::tr("Forget")).on_hover_text(crate::i18n::tr("Go back to looking where it's usually installed")).clicked() {
                *action = Some(Act::Forget(tool));
            }
        });
        ui.add_space(6.0);
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("After F9 builds a C64 program, pack a copy with:"));
        egui::ComboBox::from_id_salt("c64-after-build").selected_text(state.prefs.after_build.map_or("Nothing", Tool::name)).show_ui(ui, |ui| {
            ui.selectable_value(&mut state.prefs.after_build, None, crate::i18n::tr("Nothing"));
            for tool in Tool::PACKERS.into_iter().filter(|t| t.makes_runnable()) {
                ui.selectable_value(&mut state.prefs.after_build, Some(tool), tool.name());
            }
        });
    });
    note(ui, "The copy unpacks and starts itself, as the Pack page says to start it, and goes beside the source as name_packed.prg, in the background; the program runs as built.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{app_with, draw};

    fn file(name: &str, bytes: &[u8]) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("limoniter-c64-host-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn the_box(app: &mut Limoniter) -> &mut C64ToolsBox {
        match &mut app.dialog {
            Dialog::C64Tools(state) => state,
            _ => panic!("the box isn't open"),
        }
    }

    #[test]
    fn a_sid_tune_shows_its_header_opens_its_code_and_its_calls_go_into_6502_source() {
        let (mut app, ctx) = app_with("");
        crate::highlight::load_now();
        let source = file("demo.a65", b"\t* = $0801\nstart:\trts\n");
        assert!(app.open_path(&source));
        draw(&mut app, &ctx, Vec::new());
        let source_tab = app.active;
        let bytes = crate::sid::tests::synthetic(b"PSID", 2, 0, 0x1000, 0x1003, 3, 2, 0, 0b10_01_00, [0; 4], &[0x00, 0x10, 0x60, 0x60, 0x60, 0x60]);
        let path = file("tune.sid", &bytes);
        assert!(app.open_path(&path));
        assert!(app.docs[app.active].hex_mode);
        assert_eq!(app.docs[app.active].preview_view, crate::settings::PreviewView::Split, "a tune opens with its details showing");
        draw(&mut app, &ctx, Vec::new());
        assert!(app.preview_blocks.iter().any(|b| b.spans.first().is_some_and(|s| s.text == "Test Tune")), "the preview shows the tune");
        app.open_c64_tools();
        draw(&mut app, &ctx, Vec::new());
        let state = the_box(&mut app);
        assert_eq!(state.page, Page::Music);
        assert_eq!(state.song, 2, "the start song is chosen");
        assert_eq!(app.m6502_target(), Some(source_tab));
        let tune = crate::sid::read(&bytes).unwrap();
        app.c64_tools_act(&ctx, Act::Insert(crate::sid::replay_snippet(&tune, "tune.sid", 2)));
        let text = app.docs[source_tab].text.all_lines().join("\n");
        assert!(text.contains("sid_play = $1003") && text.contains("sid_song = 1"), "{text}");
        // The tune's code, as a program.
        app.active = app.docs.iter().position(|d| d.path.as_deref() == Some(path.as_path())).unwrap();
        app.open_c64_tools();
        app.c64_tools_act(&ctx, Act::OpenProgram);
        let opened = &app.docs[app.active];
        assert!(opened.name().ends_with("tune.prg"), "{}", opened.name());
        assert_eq!(opened.hex.bytes(), [0x00, 0x10, 0x60, 0x60, 0x60, 0x60]);
    }

    #[test]
    fn a_program_opens_the_pack_page_and_a_missing_packer_is_only_a_note() {
        let (mut app, ctx) = app_with("");
        let prg = [0x01, 0x08, 0x0C, 0x08, 0x0A, 0x00, 0x9E, b'2', b'0', b'6', b'2', 0, 0, 0, 0x60];
        let path = file("demo.prg", &prg);
        assert!(app.open_path(&path));
        app.open_c64_tools();
        draw(&mut app, &ctx, Vec::new());
        let state = the_box(&mut app);
        assert_eq!(state.page, Page::Pack);
        assert_eq!(state.output_name, "demo_packed.prg");
        state.found = Tool::ALL.into_iter().map(|t| (t, None)).collect();
        app.c64_pack(&ctx);
        let state = the_box(&mut app);
        assert!(state.job.is_none());
        assert!(matches!(&state.result, Some(Err(problem)) if problem.contains("wasn't found")));
        let id = app.docs[app.active].id;
        app.c64_pack_after_build(id, "demo.prg", &prg);
        assert!(app.c64_jobs.is_empty(), "nothing is packed after building unless the setting says");
    }

    #[test]
    fn a_cartridge_is_made_from_a_binary_and_opens_as_its_chips() {
        let (mut app, ctx) = app_with("");
        let mut binary = vec![0xEAu8; 8192];
        binary[..9].copy_from_slice(&[0x09, 0x80, 0x09, 0x80, 0xC3, 0xC2, 0xCD, 0x38, 0x30]);
        let path = file("game.bin", &binary);
        assert!(app.open_path(&path));
        app.open_c64_tools();
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(the_box(&mut app).page, Page::Cartridge);
        app.c64_tools_act(&ctx, Act::Cartridge);
        let crt = path.with_file_name("game.crt");
        assert!(matches!(&the_box(&mut app).result, Some(Ok(o)) if o.ok), "{:?}", the_box(&mut app).result.as_ref().map(|r| r.as_ref().map(|o| o.made.clone())));
        app.dialog = Dialog::None;
        assert!(app.open_path(&crt));
        let browser = app.docs[app.active].browser.as_ref().expect("a cartridge opens in the browser");
        assert!(browser.place().contains("GAME") || app.docs[app.active].name().contains("game"));
    }

    /// With exomizer named by LIMONITER_EXOMIZER (or installed), the whole way: the box packs the program in the
    /// background, and F9's build is packed beside its source when the setting says.
    #[test]
    fn exomizer_packs_from_the_box_and_after_building() {
        let Some(exomizer) = std::env::var_os("LIMONITER_EXOMIZER").map(PathBuf::from).filter(|p| p.is_file()) else {
            eprintln!("LIMONITER_EXOMIZER isn't set, so nothing was packed.");
            return;
        };
        let (mut app, ctx) = app_with("");
        let mut prg = vec![0x01, 0x08, 0x0C, 0x08, 0x0A, 0x00, 0x9E, b'2', b'0', b'6', b'2', 0, 0, 0];
        // Code that varies, as real code does: Exomizer takes minutes over long runs of the very same bytes.
        let mut seed = 12345u32;
        for _ in 0..2000 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            prg.extend([0xA9, (seed % 8) as u8, 0x8D, 0x20 + (seed >> 8) as u8 % 4, 0xD0]);
        }
        prg.push(0x60);
        let path = file("intro.prg", &prg);
        assert!(app.open_path(&path));
        app.open_c64_tools();
        let state = the_box(&mut app);
        state.prefs.set_path(Tool::Exomizer, Some(exomizer.clone()));
        state.prefs.packer = Tool::Exomizer;
        state.found = find_all(&state.prefs);
        app.c64_pack(&ctx);
        let started = std::time::Instant::now();
        while the_box(&mut app).job.is_some() && started.elapsed().as_secs() < 300 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            draw(&mut app, &ctx, Vec::new());
        }
        let outcome = the_box(&mut app).result.clone().unwrap().unwrap();
        assert!(outcome.ok, "{:?}", outcome.printed);
        let packed = path.with_file_name("intro_packed.prg");
        let bytes = std::fs::read(&packed).unwrap();
        assert_eq!(&bytes[..2], &[0x01, 0x08], "a self-unpacking program loads as BASIC");
        assert!(bytes.len() < prg.len(), "{}", outcome.sizes());
        assert!(app.script_output.iter().any(|l| l.contains("Packed in")), "{:?}", app.script_output);
        let mut prefs = ToolPrefs::load();
        prefs.after_build = Some(Tool::Exomizer);
        prefs.save();
        app.dialog = Dialog::None;
        let _ = std::fs::remove_file(&packed);
        let id = app.docs[app.active].id;
        app.c64_pack_after_build(id, "intro.prg", &prg);
        assert_eq!(app.c64_jobs.len(), 1);
        while !app.c64_jobs.is_empty() && started.elapsed().as_secs() < 600 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            app.c64_jobs_tick(&ctx);
        }
        assert!(std::fs::read(&packed).is_ok_and(|b| b.len() < prg.len()), "{:?}", app.script_output);
    }
}
