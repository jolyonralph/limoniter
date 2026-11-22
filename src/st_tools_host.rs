//! The window's side of the Atari ST music and packers box (Tools, Atari ST): SNDH and YM music in front, with
//! the replay calls for SNDH put into 68000 source; packing the file in front with ZX0, LZSA, LZ4, Shrinkler or
//! UPX in the background, with its depacking calls; and the YM2149's note tables and envelope shapes. A missing
//! tool only ever means a note.

use super::{Dialog, Limoniter, theme};
use crate::amiga_tools::{Outcome, group};
use crate::assemblers::Cpu;
use crate::st_music::{Music, Timer};
use crate::st_tools::{self as tools, Prefs, TableKind, Tool};
use eframe::egui::{self, Button, Context, Key, RichText};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Page {
    Music,
    Pack,
    Chip,
    Tools,
}

/// The file the box works on: on the disk, or taken out of an image or not saved yet.
pub(super) struct Source {
    name: String,
    path: Option<PathBuf>,
    bytes: Arc<Vec<u8>>,
    /// Where what's made goes.
    folder: Option<PathBuf>,
    music: Option<Music>,
    /// What a packed file holds.
    unpacked: Option<crate::st_packers::Unpacked>,
}

pub(super) struct StToolsBox {
    page: Page,
    prefs: Prefs,
    found: Vec<(Tool, Option<PathBuf>)>,
    source: Option<Source>,
    packer: Tool,
    output_name: String,
    tune: usize,
    timer: (Timer, u32),
    table: TableKind,
    clock: u32,
    a4: f64,
    first_octave: i32,
    octaves: i32,
    label: String,
    shape: u8,
    envelope_period: u16,
    job: Option<(String, Arc<AtomicBool>, Receiver<Result<Outcome, String>>)>,
    result: Option<Result<Outcome, String>>,
    fetching: Option<(Tool, Receiver<Result<PathBuf, String>>)>,
    status: String,
}

fn find_all(prefs: &Prefs) -> Vec<(Tool, Option<PathBuf>)> {
    Tool::ALL.into_iter().map(|t| (t, prefs.find(t))).collect()
}

fn found(found: &[(Tool, Option<PathBuf>)], tool: Tool) -> Option<PathBuf> {
    found.iter().find(|(t, _)| *t == tool).and_then(|(_, p)| p.clone())
}

/// The file in a scratch folder of its own, for a packer to read when it isn't on the disk as it is.
fn scratch_copy(name: &str, bytes: &[u8]) -> Result<(PathBuf, PathBuf), String> {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let folder = std::env::temp_dir().join(format!("limoniter-st-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).map_err(|e| crate::i18n::tf!("A folder to work in couldn't be made: {e}"))?;
    let safe = name.rsplit(['/', '\\', ':', '\u{203A}']).next().unwrap_or(name).trim();
    let path = folder.join(if safe.is_empty() { "file" } else { safe });
    std::fs::write(&path, bytes).map_err(|e| crate::i18n::tf!("{} couldn't be written: {e}", path.display()))?;
    Ok((folder, path))
}

/// A file's own name, whatever place it came from: "disk.st › TUNE.SND" is TUNE.SND.
fn own_name(name: &str) -> String {
    name.rsplit(['/', '\\', '\u{203A}']).next().unwrap_or(name).trim().to_string()
}

impl Limoniter {
    /// The file the box is for: the file chosen in a browser tab, or the document in front.
    fn st_tools_source(&self) -> Option<Source> {
        if self.no_file() {
            return None;
        }
        let doc = &self.docs[self.active];
        let (name, path, bytes, folder) = if let Some(browser) = &doc.browser {
            let (name, bytes) = browser.selected_file()?.ok()?;
            (name, None, bytes, doc.path.as_deref().and_then(Path::parent).map(Path::to_path_buf))
        } else {
            let bytes = if doc.hex_mode {
                doc.hex.bytes().to_vec()
            } else if let Some(path) = doc.path.as_deref().filter(|_| !doc.is_modified()) {
                std::fs::read(path).ok()?
            } else {
                doc.text.encode(&doc.text.format, crate::codec::Lossy::Omit).ok()?
            };
            let path = doc.path.clone().filter(|_| !doc.is_modified());
            (doc.name(), path, bytes, doc.path.as_deref().and_then(Path::parent).map(Path::to_path_buf))
        };
        let unpacked = crate::st_packers::unpack(&bytes).and_then(Result::ok);
        let music = crate::st_music::read(&bytes, &name).ok();
        Some(Source { name, path, bytes: Arc::new(bytes), folder, music, unpacked })
    }

    /// Opens the box (Tools, Atari ST), on the page that suits the file in front.
    pub(super) fn open_st_tools(&mut self) {
        let prefs = Prefs::load();
        let source = self.st_tools_source();
        let page = match &source {
            Some(s) if s.music.is_some() => Page::Music,
            Some(_) => Page::Pack,
            None => Page::Chip,
        };
        let (tune, timer) = match source.as_ref().and_then(|s| s.music.as_ref()) {
            Some(Music::Sndh(sndh)) => (sndh.default_tune.unwrap_or(1).clamp(1, sndh.tunes.len().max(1)), sndh.timer_or_default()),
            _ => (1, (Timer::C, 50)),
        };
        let packer = match &source {
            Some(s) if tools::is_tos_program(&s.bytes) => Tool::Upx,
            _ => Tool::Salvador,
        };
        let output_name = source.as_ref().map(|s| packer.packed_name(&own_name(&s.name))).unwrap_or_default();
        let found = find_all(&prefs);
        self.dialog = Dialog::StTools(Box::new(StToolsBox {
            page,
            prefs,
            found,
            source,
            packer,
            output_name,
            tune,
            timer,
            table: TableKind::Tone,
            clock: 2_000_000,
            a4: 440.0,
            first_octave: 1,
            octaves: 7,
            label: "ym_periods".into(),
            shape: 14,
            envelope_period: 300,
            job: None,
            result: None,
            fetching: None,
            status: String::new(),
        }));
    }

    /// The 68000 source a snippet goes into: the document in front if it's 68000 source, else the first open one.
    fn st_m68k_target(&self) -> Option<usize> {
        if self.doc_cpu(self.active) == Some(Cpu::M68k) {
            return Some(self.active);
        }
        (0..self.docs.len()).find(|&i| self.doc_cpu(i) == Some(Cpu::M68k) && !self.docs[i].text.read_only)
    }

    pub(super) fn st_tools_dialog(&mut self, ctx: &Context) -> bool {
        let target = self.st_m68k_target().map(|i| self.docs[i].name());
        let Dialog::StTools(state) = &mut self.dialog else {
            return false;
        };
        let mut printed: Vec<String> = Vec::new();
        if let Some((what, _, rx)) = &state.job {
            match rx.try_recv() {
                Ok(result) => {
                    printed.push(what.clone());
                    match &result {
                        Ok(outcome) => {
                            printed.extend(outcome.printed.iter().cloned());
                            if outcome.ok {
                                printed.push(crate::i18n::tf!("Packed in {:.1} s: {}.", outcome.seconds, outcome.sizes()));
                            } else {
                                printed.push(crate::i18n::tr("It didn't finish: see what it printed above.").into());
                            }
                        }
                        Err(problem) => printed.push(problem.clone()),
                    }
                    state.result = Some(result);
                    state.job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => state.job = None,
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(200)),
            }
        }
        if let Some((tool, rx)) = &state.fetching {
            match rx.try_recv() {
                Ok(Ok(path)) => {
                    state.status = crate::i18n::tf!("{} is ready at {}.", tool.name(), path.display());
                    state.found = find_all(&state.prefs);
                    state.fetching = None;
                }
                Ok(Err(problem)) => {
                    state.status = problem;
                    state.fetching = None;
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(200)),
            }
        }
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut action: Option<Act> = None;
        let busy = state.job.is_some();
        crate::fit::Dialog::new(crate::i18n::tr("Atari ST music and packers")).collapsible(false).resizable(true).default_width(620.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(660.0);
            ui.horizontal(|ui| {
                for (page, label) in [(Page::Music, "SNDH and YM music"), (Page::Pack, "Pack"), (Page::Chip, "YM2149 notes and envelopes"), (Page::Tools, "Tools")] {
                    if ui.selectable_label(state.page == page, crate::i18n::tr(label)).clicked() {
                        state.page = page;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(540.0).show(ui, |ui| match state.page {
                Page::Music => music_page(ui, state, target.as_deref(), &mut action),
                Page::Pack => pack_page(ui, state, target.as_deref(), &mut action),
                Page::Chip => chip_page(ui, state, target.as_deref(), &mut action),
                Page::Tools => tools_page(ui, state, &mut action),
            });
            ui.separator();
            if let Some((what, stop, _)) = &state.job {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(crate::i18n::tf!("{what}\u{2026}"));
                    if ui.button(crate::i18n::tr("Stop")).clicked() {
                        stop.store(true, Ordering::Relaxed);
                    }
                });
            } else if let Some(result) = &state.result {
                match result {
                    Ok(outcome) if outcome.ok => {
                        ui.colored_label(theme::change_saved(), crate::i18n::tf!("Done: {}.", outcome.sizes()));
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
            close |= ui.add_enabled(!busy, Button::new(crate::i18n::tr("Close"))).clicked();
        });
        if let Some(act) = action {
            self.st_tools_act(ctx, act);
        }
        if let Dialog::StTools(state) = &self.dialog
            && state.prefs != Prefs::load()
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
    fn st_tools_act(&mut self, ctx: &Context, act: Act) {
        let target = self.st_m68k_target().map(|i| (i, self.docs[i].name()));
        match act {
            Act::Pack => self.st_pack(ctx),
            Act::Insert(text) => match target {
                Some((index, name)) => {
                    self.docs[index].text.paste_text(&text, false);
                    self.active = index;
                    if let Dialog::StTools(state) = &mut self.dialog {
                        state.status = crate::i18n::tf!("Put into {name}, at the cursor.");
                    }
                }
                None => ctx.copy_text(text),
            },
            Act::Copy(text) => ctx.copy_text(text),
            Act::SaveUnpacked => {
                let Dialog::StTools(state) = &mut self.dialog else {
                    return;
                };
                let Some((name, bytes)) = state.source.as_ref().and_then(|s| s.unpacked.as_ref().map(|u| (own_name(&s.name), u.bytes.clone()))) else {
                    return;
                };
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save what it holds, unpacked")).set_file_name(name).save_file() {
                    state.status = match std::fs::write(&path, &bytes) {
                        Ok(()) => crate::i18n::tf!("Saved {} ({} bytes).", path.display(), group(bytes.len() as u64)),
                        Err(e) => crate::i18n::tf!("{} couldn't be saved: {e}", path.display()),
                    };
                }
            }
            Act::Fetch(tool) => {
                if let Dialog::StTools(state) = &mut self.dialog {
                    let (tx, rx) = channel();
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(tools::fetch(tool));
                        ctx.request_repaint();
                    });
                    state.status = format!("Fetching {}\u{2026}", tool.name());
                    state.fetching = Some((tool, rx));
                }
            }
            Act::Choose(tool) => {
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tf!("Where is {}?", tool.name())).pick_file()
                    && let Dialog::StTools(state) = &mut self.dialog
                {
                    state.prefs.set_path(tool, Some(path));
                    state.found = find_all(&state.prefs);
                }
            }
            Act::Forget(tool) => {
                if let Dialog::StTools(state) = &mut self.dialog {
                    state.prefs.set_path(tool, None);
                    state.found = find_all(&state.prefs);
                }
            }
            Act::Folder => {
                if let Some(folder) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Where to put what's made")).pick_folder()
                    && let Dialog::StTools(state) = &mut self.dialog
                    && let Some(source) = &mut state.source
                {
                    source.folder = Some(folder);
                }
            }
        }
    }

    fn st_pack(&mut self, ctx: &Context) {
        let Dialog::StTools(state) = &mut self.dialog else {
            return;
        };
        let Some(source) = &state.source else {
            return;
        };
        let tool = state.packer;
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
        if tool == Tool::Upx && !tools::is_tos_program(&source.bytes) {
            state.result = Some(Err(crate::i18n::tr("UPX packs TOS programs (.prg, .tos, .ttp), and this isn't one.").into()));
            return;
        }
        let prefs = state.prefs.clone();
        let (path, bytes, name) = (source.path.clone(), source.bytes.clone(), own_name(&source.name));
        let what = crate::i18n::tf!("Packing {name} with {}", tool.name());
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (flag, ctx) = (stop.clone(), ctx.clone());
        std::thread::spawn(move || {
            let work = || {
                let (scratch, input) = match path {
                    Some(path) => (None, path),
                    None => {
                        let (folder, path) = scratch_copy(&name, &bytes)?;
                        (Some(folder), path)
                    }
                };
                let result = tools::pack(tool, &program, &prefs, &input, &output, &flag);
                if let Some(folder) = scratch {
                    let _ = std::fs::remove_dir_all(folder);
                }
                result
            };
            let _ = tx.send(work());
            ctx.request_repaint();
        });
        state.result = None;
        state.job = Some((what, stop, rx));
    }
}

pub(super) enum Act {
    Pack,
    Insert(String),
    Copy(String),
    SaveUnpacked,
    Fetch(Tool),
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
        ui.label(crate::i18n::tf!("{} bytes", group(source.bytes.len() as u64)));
    });
    if let Some(unpacked) = &source.unpacked {
        ui.horizontal_wrapped(|ui| {
            note(ui, crate::i18n::tf!("Packed with {}: it holds {} bytes.", unpacked.packer.name(), group(unpacked.bytes.len() as u64)));
            if ui.small_button(crate::i18n::tr("Save what it holds\u{2026}")).clicked() {
                *action = Some(Act::SaveUnpacked);
            }
        });
    }
}

fn folder_line(ui: &mut egui::Ui, source: &Source, action: &mut Option<Act>) {
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

/// The snippet, shown, with buttons to put it into 68000 source or copy it.
fn snippet_box(ui: &mut egui::Ui, snippet: &str, target: Option<&str>, action: &mut Option<Act>) {
    let mut shown = snippet;
    ui.add(egui::TextEdit::multiline(&mut shown).code_editor().desired_rows(10).desired_width(f32::INFINITY));
    ui.horizontal(|ui| {
        match target {
            Some(name) => {
                if ui.button(crate::i18n::tf!("Insert into {name}")).on_hover_text(crate::i18n::tr("At the cursor, as one change to undo")).clicked() {
                    *action = Some(Act::Insert(snippet.to_string()));
                }
            }
            None => note(ui, "Open 68000 source to insert it there."),
        }
        if ui.button(crate::i18n::tr("Copy")).clicked() {
            *action = Some(Act::Copy(snippet.to_string()));
        }
    });
}

fn music_page(ui: &mut egui::Ui, state: &mut StToolsBox, target: Option<&str>, action: &mut Option<Act>) {
    let Some(source) = &state.source else {
        ui.label(crate::i18n::tr("Open SNDH or YM music (packed or not), or choose it in a disk image's browser, then come back here."));
        return;
    };
    file_line(ui, source, action);
    let Some(music) = &source.music else {
        ui.colored_label(theme::rainbow_mistake(), crate::i18n::tr("This isn't SNDH or YM music."));
        return;
    };
    for section in music.sections() {
        ui.add_space(4.0);
        ui.strong(&section.title);
        egui::Grid::new(format!("st-music-{}", section.title)).num_columns(2).striped(true).show(ui, |ui| {
            for (label, value) in &section.rows {
                ui.label(RichText::new(label).color(theme::text_dim()));
                ui.label(value);
                ui.end_row();
            }
        });
    }
    let Music::Sndh(sndh) = music else {
        ui.add_space(6.0);
        note(ui, "A YM file is the chip's registers frame by frame, for a YM player such as ST-Sound to play; it has no replay routine of its own to call.");
        return;
    };
    ui.add_space(6.0);
    ui.strong("Replay calls");
    note(ui, "Written by Limoniter for you to adapt: they call the file's own init, play and exit, and include it.");
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Tune:"));
        ui.add(egui::DragValue::new(&mut state.tune).range(1..=sndh.tunes.len().max(1)));
        if let Some(name) = sndh.tunes.get(state.tune.saturating_sub(1)).and_then(|t| t.name.as_ref()) {
            note(ui, name.clone());
        }
    });
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Played from:"));
        egui::ComboBox::from_id_salt("st-timer").selected_text(crate::i18n::tr(state.timer.0.name())).show_ui(ui, |ui| {
            for timer in [Timer::A, Timer::B, Timer::C, Timer::D, Timer::Vbl] {
                ui.selectable_value(&mut state.timer.0, timer, crate::i18n::tr(timer.name()));
            }
        });
        ui.add(egui::DragValue::new(&mut state.timer.1).range(1..=20_000).suffix(" Hz"));
        let asked = sndh.timer_or_default();
        if state.timer != asked && ui.small_button(crate::i18n::tf!("As the file asks ({} at {} Hz)", asked.0.name(), asked.1)).clicked() {
            state.timer = asked;
        }
    });
    let file_name = own_name(&source.name);
    let snippet = crate::st_music::replay_snippet(sndh, &file_name, state.tune, state.timer);
    snippet_box(ui, &snippet, target, action);
}

fn pack_page(ui: &mut egui::Ui, state: &mut StToolsBox, target: Option<&str>, action: &mut Option<Act>) {
    let Some(source) = &state.source else {
        ui.label(crate::i18n::tr("Open the program or data to pack (it can be in hex), or choose it in a disk image's browser, then come back here."));
        return;
    };
    file_line(ui, source, action);
    folder_line(ui, source, action);
    let program = tools::is_tos_program(&source.bytes);
    note(ui, if program { "This is a TOS program." } else { "This is data: it's packed as it is, for your own code to unpack." });
    ui.add_space(4.0);
    let before = state.packer;
    ui.horizontal_wrapped(|ui| {
        for tool in Tool::ALL {
            ui.radio_value(&mut state.packer, tool, crate::i18n::tr(tool.name()));
        }
    });
    if state.packer != before {
        state.output_name = state.packer.packed_name(&own_name(&source.name));
    }
    let ready = match found(&state.found, state.packer) {
        Some(path) => {
            note(ui, format!("{} is at {}", state.packer.name(), path.display()));
            true
        }
        None => {
            ui.colored_label(theme::rainbow_mistake(), crate::i18n::tf!("{} wasn't found.", state.packer.name()));
            note(ui, state.packer.how_to_get());
            false
        }
    };
    let prefs = &mut state.prefs;
    match state.packer {
        Tool::Zx0 => {
            ui.checkbox(&mut prefs.zx0_quick, crate::i18n::tr("Quick, packing less well (-q)"));
        }
        Tool::Lzsa => {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Format:"));
                ui.radio_value(&mut prefs.lzsa_format, 1, crate::i18n::tr("LZSA1 (unpacks faster)"));
                ui.radio_value(&mut prefs.lzsa_format, 2, crate::i18n::tr("LZSA2 (packs smaller)"));
            });
        }
        Tool::Lz4 => {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Level:"));
                ui.add(egui::Slider::new(&mut prefs.lz4_level, 1..=12));
            });
        }
        Tool::Shrinkler => {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Effort:"));
                ui.add(egui::Slider::new(&mut prefs.shrinkler_preset, 1..=9));
            });
            note(ui, "Shrinkler packs smallest of all, but unpacks slowly: best for an intro's data, unpacked once.");
        }
        Tool::Upx => {
            ui.checkbox(&mut prefs.upx_best, crate::i18n::tr("Best (--best: slower, smaller)"));
            if !program {
                ui.colored_label(theme::rainbow_mistake(), crate::i18n::tr("UPX packs TOS programs only."));
            }
        }
        Tool::Salvador => note(ui, "salvador packs ZX0 a little smaller than zx0 itself, and checks it unpacks (-c)."),
    }
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Save as:"));
        ui.text_edit_singleline(&mut state.output_name);
    });
    if ui.add_enabled(ready && state.job.is_none(), Button::new(crate::i18n::tf!("Pack with {}", state.packer.name()))).clicked() {
        *action = Some(Act::Pack);
    }
    ui.add_space(6.0);
    ui.strong("Unpacking it on the ST");
    let (file, from, terms) = state.packer.depacker();
    note(ui, format!("Depacker: {file}, {terms}: {from}"));
    let size = source.unpacked.as_ref().map_or(source.bytes.len(), |u| u.bytes.len()) as u64;
    let size = if source.unpacked.is_some() { source.bytes.len() as u64 } else { size };
    let snippet = tools::depack_snippet(state.packer, state.output_name.trim(), size);
    snippet_box(ui, &snippet, target, action);
}

/// Draws an envelope shape: four cycles of its level, from silent to loud.
fn draw_shape(ui: &mut egui::Ui, shape: u8, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(92.0, 40.0), egui::Sense::click());
    let painter = ui.painter_at(rect);
    let back = if selected { theme::selected() } else if response.hovered() { theme::hover() } else { theme::field() };
    painter.rect_filled(rect, 4.0, back);
    let inner = rect.shrink2(egui::vec2(6.0, 8.0));
    let y = |level: u8| inner.bottom() - inner.height() * f32::from(level) / 15.0;
    let width = inner.width() / 4.0;
    let stroke = egui::Stroke::new(1.6, if selected { theme::strong_text() } else { theme::accent() });
    let mut last: Option<egui::Pos2> = None;
    for (i, (start, end)) in tools::envelope_shape(shape).iter().enumerate() {
        let a = egui::pos2(inner.left() + width * i as f32, y(*start));
        let b = egui::pos2(a.x + width, y(*end));
        if let Some(previous) = last {
            painter.line_segment([previous, a], stroke);
        }
        painter.line_segment([a, b], stroke);
        last = Some(b);
    }
    painter.text(rect.left_top() + egui::vec2(3.0, 1.0), egui::Align2::LEFT_TOP, shape.to_string(), egui::FontId::proportional(10.0), theme::text_dim());
    response.on_hover_text(crate::i18n::tf!("Shape {shape} (register 13 = {shape}): {}", tools::envelope_words(shape)))
}

fn chip_page(ui: &mut egui::Ui, state: &mut StToolsBox, target: Option<&str>, action: &mut Option<Act>) {
    ui.strong("Note table");
    ui.horizontal(|ui| {
        ui.radio_value(&mut state.table, TableKind::Tone, crate::i18n::tr("YM2149 tone periods"));
        ui.radio_value(&mut state.table, TableKind::Buzzer, crate::i18n::tr("Envelope periods (buzzer)"));
        ui.radio_value(&mut state.table, TableKind::Amiga, crate::i18n::tr("Amiga (ProTracker's)"));
    });
    if state.table != TableKind::Amiga {
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Clock:"));
            let shown = tools::CLOCKS.iter().find(|(_, c)| *c == state.clock).map_or_else(|| format!("{} Hz", group(u64::from(state.clock))), |(n, _)| n.to_string());
            egui::ComboBox::from_id_salt("ym-clock").selected_text(shown).show_ui(ui, |ui| {
                for (name, clock) in tools::CLOCKS {
                    ui.selectable_value(&mut state.clock, clock, crate::i18n::tr(name));
                }
            });
            ui.label(crate::i18n::tr("A-4 ="));
            ui.add(egui::DragValue::new(&mut state.a4).range(400.0..=480.0).speed(0.1).suffix(" Hz"));
        });
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Octaves"));
            ui.add(egui::DragValue::new(&mut state.first_octave).range(0..=8));
            ui.label(crate::i18n::tr("to"));
            let mut last = state.first_octave + state.octaves - 1;
            ui.add(egui::DragValue::new(&mut last).range(state.first_octave..=9));
            state.octaves = last - state.first_octave + 1;
        });
    }
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Label:"));
        ui.add(egui::TextEdit::singleline(&mut state.label).desired_width(160.0));
    });
    let table = tools::period_table(state.table, state.clock, state.a4, state.first_octave, state.octaves, &state.label);
    snippet_box(ui, &table, target, action);
    ui.add_space(8.0);
    ui.strong("Envelope shapes");
    note(ui, "Register 13 chooses the shape, and writing it starts the envelope again. 0 to 7 each sound as 9 or 15.");
    egui::Grid::new("ym-shapes").spacing([6.0, 6.0]).show(ui, |ui| {
        for row in 0..4u8 {
            for column in 0..4u8 {
                let shape = row * 4 + column;
                if draw_shape(ui, shape, shape == state.shape).clicked() {
                    state.shape = shape;
                }
            }
            ui.end_row();
        }
    });
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tf!("Shape {}: {}.", state.shape, tools::envelope_words(state.shape)));
        ui.label(crate::i18n::tr("Period:"));
        ui.add(egui::DragValue::new(&mut state.envelope_period).range(1..=65535));
    });
    let snippet = tools::envelope_snippet(state.shape, state.envelope_period, if state.table == TableKind::Amiga { 2_000_000 } else { state.clock });
    snippet_box(ui, &snippet, target, action);
}

fn tools_page(ui: &mut egui::Ui, state: &mut StToolsBox, action: &mut Option<Act>) {
    note(ui, "Limoniter runs these where they're installed, or where you point it. It never fetches or builds one unless you ask.");
    ui.add_space(4.0);
    for tool in Tool::ALL {
        ui.horizontal_wrapped(|ui| {
            ui.strong(tool.name());
            ui.hyperlink_to("website", tool.website());
        });
        note(ui, tool.licence());
        let (file, from, terms) = tool.depacker();
        note(ui, crate::i18n::tf!("Unpacked on the ST by {file} ({terms}), {from}."));
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
            let fetch = if tool.release().is_some() {
                Some(("Download its release", tool.release().map_or("", |r| r.0)))
            } else if tool.source().is_some() {
                Some(("Build it from its source", tool.source().map_or("", |s| s.0)))
            } else {
                None
            };
            if let Some((label, from)) = fetch
                && ui.add_enabled(state.fetching.is_none(), Button::new(label).small()).on_hover_text(from).clicked()
            {
                *action = Some(Act::Fetch(tool));
            }
        });
        ui.add_space(6.0);
    }
    note(ui, "Pack-Ice and Atomik files aren't packed here (their packers are ST programs), but Limoniter unpacks them wherever it opens one.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{app_with, draw};

    fn file(name: &str, bytes: &[u8]) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("limoniter-st-host-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn the_box(app: &mut Limoniter) -> &mut StToolsBox {
        match &mut app.dialog {
            Dialog::StTools(state) => state,
            _ => panic!("the box isn't open"),
        }
    }

    #[test]
    fn packed_sndh_opens_unpacked_with_its_details_and_its_replay_calls_go_into_68000_source() {
        let (mut app, ctx) = app_with("");
        crate::highlight::load_now();
        let source = file("demo.s68", b"\tsection\ttext\nstart:\tmoveq\t#0,d0\n\trts\n");
        assert!(app.open_path(&source));
        draw(&mut app, &ctx, Vec::new());
        let source_tab = app.active;
        let sndh = crate::st_music::tests::synthetic(b"TITLIced tune\0COMMSomeone\0##02TC50\0");
        let packed = crate::st_packers::tests::ice_pack(&sndh, 1);
        let path = file("TUNE.SND", &packed);
        assert!(app.open_path(&path));
        let doc = &app.docs[app.active];
        assert!(doc.hex_mode, "opened in hex");
        assert_eq!(doc.hex.bytes()[12..16], *b"SNDH", "what it holds, not the packed bytes");
        assert!(doc.name().contains("TUNE.SND"), "{}", doc.name());
        assert_eq!(app.docs[app.active].preview_view, crate::settings::PreviewView::Split, "music opens with its details showing");
        draw(&mut app, &ctx, Vec::new());
        assert!(app.preview_blocks.iter().any(|b| b.spans.first().is_some_and(|s| s.text == "Iced tune")), "the preview shows the music");
        app.open_st_tools();
        draw(&mut app, &ctx, Vec::new());
        let state = the_box(&mut app);
        assert_eq!(state.page, Page::Music);
        assert_eq!(state.timer, (Timer::C, 50));
        assert_eq!(app.st_m68k_target(), Some(source_tab));
        let Some(Music::Sndh(read)) = &the_box(&mut app).source.as_ref().unwrap().music else { panic!("not read as SNDH") };
        let snippet = crate::st_music::replay_snippet(read, "TUNE.SND", 2, (Timer::C, 50));
        app.st_tools_act(&ctx, Act::Insert(snippet));
        let text = app.docs[source_tab].text.all_lines().join("\n");
        assert!(text.contains("moveq\t#2,d0") && text.contains("incbin\t\"TUNE.SND\""), "{text}");
        // The other pages draw.
        for page in [Page::Pack, Page::Chip, Page::Tools] {
            the_box(&mut app).page = page;
            draw(&mut app, &ctx, Vec::new());
        }
    }

    /// A ZIP archive of files stored as they are.
    fn stored_zip(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let (mut out, mut directory) = (Vec::new(), Vec::new());
        for (name, bytes) in files {
            let at = out.len() as u32;
            let sizes = [(bytes.len() as u32).to_le_bytes(), (bytes.len() as u32).to_le_bytes()].concat();
            out.extend(b"PK\x03\x04\x14\0\0\0\0\0\0\0\0\0\0\0\0\0");
            out.extend(&sizes);
            out.extend((name.len() as u16).to_le_bytes());
            out.extend([0, 0]);
            out.extend(name.as_bytes());
            out.extend(bytes);
            directory.extend(b"PK\x01\x02\x14\0\x14\0\0\0\0\0\0\0\0\0\0\0\0\0");
            directory.extend(&sizes);
            directory.extend((name.len() as u16).to_le_bytes());
            directory.extend([0u8; 12]);
            directory.extend(at.to_le_bytes());
            directory.extend(name.as_bytes());
        }
        let start = out.len() as u32;
        out.extend(&directory);
        out.extend(b"PK\x05\x06\0\0\0\0");
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((directory.len() as u32).to_le_bytes());
        out.extend(start.to_le_bytes());
        out.extend([0, 0]);
        out
    }

    #[test]
    fn a_packed_file_in_an_archive_opens_as_what_it_holds() {
        let (mut app, ctx) = app_with("");
        let sndh = crate::st_music::tests::synthetic(b"TITLIn a zip\0TC50\0");
        let zip = stored_zip(&[("MUSIC.SND", crate::st_packers::tests::ice_pack(&sndh, 1))]);
        let path = file("disk.zip", &zip);
        assert!(app.open_path(&path));
        let browser = app.active;
        assert!(app.docs[browser].browser.is_some());
        app.open_from_browser(browser, 0);
        assert_ne!(app.active, browser);
        let doc = &app.docs[app.active];
        assert_eq!(doc.hex.bytes(), sndh, "unpacked as it was opened");
        draw(&mut app, &ctx, Vec::new());
        assert!(app.preview_blocks.iter().any(|b| b.spans.first().is_some_and(|s| s.text == "In a zip")), "the preview shows the music");
    }

    #[test]
    fn a_missing_packer_is_only_a_note() {
        let (mut app, ctx) = app_with("");
        let path = file("pic.pi1", &[0x55u8; 32_034]);
        assert!(app.open_path(&path));
        app.open_st_tools();
        draw(&mut app, &ctx, Vec::new());
        let state = the_box(&mut app);
        assert_eq!(state.page, Page::Pack);
        assert_eq!(state.output_name, "pic.pi1.zx0");
        state.found = Tool::ALL.into_iter().map(|t| (t, None)).collect();
        app.st_pack(&ctx);
        let state = the_box(&mut app);
        assert!(state.job.is_none());
        assert!(matches!(&state.result, Some(Err(problem)) if problem.contains("wasn't found")));
    }

    /// With LZ4 named by LIMONITER_LZ4, the whole way: the box packs the file in the background.
    #[test]
    fn lz4_packs_from_the_box() {
        let Some(lz4) = std::env::var_os("LIMONITER_LZ4").map(PathBuf::from).filter(|p| p.is_file()) else {
            eprintln!("LIMONITER_LZ4 isn't set, so nothing was packed.");
            return;
        };
        let (mut app, ctx) = app_with("");
        let data: Vec<u8> = (0..30_000u32).map(|i| (i % 97) as u8).collect();
        let path = file("level.dat", &data);
        assert!(app.open_path(&path));
        app.open_st_tools();
        let state = the_box(&mut app);
        state.packer = Tool::Lz4;
        state.output_name = "level.dat.lz4".into();
        state.prefs.set_path(Tool::Lz4, Some(lz4));
        state.found = find_all(&state.prefs);
        app.st_pack(&ctx);
        let started = std::time::Instant::now();
        while the_box(&mut app).job.is_some() && started.elapsed().as_secs() < 60 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            draw(&mut app, &ctx, Vec::new());
        }
        let outcome = the_box(&mut app).result.clone().unwrap().unwrap();
        assert!(outcome.ok, "{:?}", outcome.printed);
        let packed = std::fs::read(path.with_file_name("level.dat.lz4")).unwrap();
        assert_eq!(packed[..4], [0x04, 0x22, 0x4D, 0x18], "an LZ4 frame");
        assert!(app.script_output.iter().any(|l| l.contains("Packed in")), "{:?}", app.script_output);
    }
}
