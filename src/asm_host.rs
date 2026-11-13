//! The window's side of assembling: source in front is assembled when it's run (F9), with Limoniter's own
//! assembler or the one chosen; problems go in the Output panel, where clicking one goes to it, and are marked
//! beside their lines. The program assembled is kept, so the debugger knows which line is at which address.

use super::*;
use crate::asm::Assembly;
use crate::assemblers::{self, Assembled, Choice, Cpu};
use crate::running::{Format, Machine, Runnable};
use std::sync::Arc;

/// The last program assembled, kept until the source changes.
pub(super) struct AssembledCache {
    pub doc_id: u64,
    version: u64,
    choice: String,
    pub runnable: Result<Runnable, String>,
    pub assembly: Arc<Assembly>,
}

/// The box choosing an assembler.
pub(super) struct AssemblerBox {
    pub cpu: Cpu,
    pub found: Vec<(&'static assemblers::External, PathBuf)>,
    pub chosen: Choice,
}

impl Limoniter {
    /// The processor the document's source is for, if it's assembler source.
    pub(super) fn doc_cpu(&self, index: usize) -> Option<Cpu> {
        let doc = self.docs.get(index)?;
        if doc.hex_mode || doc.browser.is_some() || doc.picture.is_some() {
            return None;
        }
        Cpu::for_language(&doc.language_name())
    }

    pub(super) fn assembler_for(&self, cpu: Cpu) -> Choice {
        self.running.assemblers.iter().find(|(id, _)| id == cpu.id()).map_or(Choice::Own, |(_, setting)| Choice::from_setting(setting))
    }

    pub(super) fn set_assembler(&mut self, cpu: Cpu, choice: &Choice) {
        self.running.assemblers.retain(|(id, _)| id != cpu.id());
        self.running.assemblers.push((cpu.id().to_string(), choice.setting()));
        self.running.assembled = None;
        self.save_settings_if_changed();
    }

    /// Gives assembler source its marks (breakpoints and problems), and takes them from what isn't.
    pub(super) fn assembly_tick(&mut self) {
        if self.no_file() {
            return;
        }
        let wanted = self.doc_cpu(self.active).is_some();
        let doc = &mut self.docs[self.active];
        match (wanted, doc.text.code.is_some()) {
            (true, false) => doc.text.code = Some(Box::default()),
            (false, true) => doc.text.code = None,
            _ => {}
        }
    }

    /// The program the source in front assembles to, ready to run. With `report`, it's assembled whatever it takes
    /// (saving first for another program to read) and the problems are shown; without, only Limoniter's own
    /// assembler is used, quietly, to say what the program is for. An empty error means the problems are shown.
    pub(super) fn assemble_to_run(&mut self, cpu: Cpu, report: bool) -> Result<Runnable, String> {
        let choice = self.assembler_for(cpu);
        let doc = &self.docs[self.active];
        let (doc_id, version) = (doc.id, doc.text.version());
        if let Some(cache) = &self.running.assembled
            && cache.doc_id == doc_id
            && cache.version == version
            && cache.choice == choice.setting()
            && (!report || cache.runnable.is_ok())
        {
            return cache.runnable.clone();
        }
        let path = std::path::absolute(doc.path_or_name()).unwrap_or_else(|_| doc.path_or_name());
        let stem = path.file_stem().map_or_else(|| "program".to_string(), |s| s.to_string_lossy().into_owned());
        let assembled = match &choice {
            Choice::Own => {
                let text = doc.text.all_lines().iter().map(|l| l.as_ref()).collect::<Vec<&str>>().join("\n");
                assemblers::assemble_own(cpu, &path, &text)
            }
            Choice::Program { id, path: program } if report => {
                let found = assemblers::external(id).ok_or("That assembler isn't known.")?;
                if doc.path.is_none() {
                    return Err(format!("Save the file first, so {} can read it.", found.name));
                }
                if doc.is_modified() {
                    let index = self.active;
                    if !self.save(index) {
                        return Err("The file couldn't be saved, so it wasn't assembled.".into());
                    }
                    if self.docs[index].saving.is_some() {
                        return Err("The file is being saved. Run it again once that's finished.".into());
                    }
                }
                let version = self.docs[self.active].text.version();
                let assembled = assemblers::assemble_external(found, program, &path)?;
                return self.keep_assembled(doc_id, version, &choice, assembled, &stem, report);
            }
            Choice::Program { .. } => return Err("Not assembled yet.".into()),
        };
        self.keep_assembled(doc_id, version, &choice, assembled, &stem, report)
    }

    fn keep_assembled(&mut self, doc_id: u64, version: u64, choice: &Choice, assembled: Assembled, stem: &str, report: bool) -> Result<Runnable, String> {
        let cpu = self.doc_cpu(self.active).unwrap_or(Cpu::Z80);
        let runnable = if assembled.assembly.ok() { runnable_from(cpu, &assembled, stem) } else { Err(String::new()) };
        if report {
            self.report_assembly(doc_id, choice, &assembled, &runnable);
        }
        let assembly = Arc::new(assembled.assembly);
        self.running.assembled = Some(AssembledCache { doc_id, version, choice: choice.setting(), runnable: runnable.clone(), assembly });
        runnable
    }

    /// The problems in the Output panel and beside their lines; or, if there are none, what was made.
    fn report_assembly(&mut self, doc_id: u64, choice: &Choice, assembled: &Assembled, runnable: &Result<Runnable, String>) {
        let assembly = &assembled.assembly;
        let Some(index) = self.docs.iter().position(|d| d.id == doc_id) else {
            return;
        };
        let name = self.docs[index].name();
        let mut lines = vec![format!("Assembling {name} with {}", choice.name())];
        if !assembly.ok() {
            // What the other assembler printed, as it printed it, besides the problems read from it.
            lines.extend(assembled.printed.iter().filter(|l| assemblers::problem_line(l).is_none() && !l.trim().is_empty()).cloned());
        }
        for problem in &assembly.problems {
            let file = assembly.files.get(problem.file).map(|f| f.display().to_string()).unwrap_or_else(|| name.clone());
            lines.push(format!("{file}({}): {}", problem.line + 1, problem.message));
        }
        lines.extend(assembly.notes.iter().map(|n| format!("> {n}")));
        match runnable {
            Ok(run) => {
                let (low, high) = (assembly.lowest().unwrap_or(0), assembly.highest().unwrap_or(0));
                let size = if assembled.tape.is_some() { "A tape was made".to_string() } else { format!("{} bytes from ${low:04X} to ${:04X}, starting at ${:04X}", high - low, (high - 1).max(low), assembly.start().unwrap_or(low)) };
                lines.push(format!("{size}: {} for the {}.", run.name, run.machine.name()));
            }
            Err(problem) if !problem.is_empty() => lines.push(problem.clone()),
            Err(_) => lines.push(format!("{} problem{}.", assembly.problems.len(), if assembly.problems.len() == 1 { "" } else { "s" })),
        }
        for (n, line) in lines.into_iter().enumerate() {
            self.script_print(line, n == 0);
        }
        let main_file = assembly.files.first().cloned();
        let doc = &mut self.docs[index];
        let code = doc.text.code.get_or_insert_with(Box::default);
        code.problems = assembly.problems.iter().filter(|p| p.file == 0 && main_file.is_some()).map(|p| (p.line, p.message.clone())).collect();
        if let Some((line, _)) = code.problems.first().cloned() {
            doc.text.move_caret(line, 0, false);
        }
    }

    /// Goes to a place named in the Output panel: a file and a line, from 1.
    pub(super) fn go_to_place(&mut self, file: &str, line: usize) {
        let named = Path::new(file);
        let found = self.docs.iter().position(|d| match &d.path {
            Some(p) => same_path(p, named) || (!named.is_absolute() && p.file_name() == named.file_name()),
            None => d.name() == file,
        });
        let index = match found {
            Some(i) => i,
            None => {
                let base = self.docs.get(self.active).and_then(|d| d.path.as_deref()).and_then(Path::parent).map(Path::to_path_buf);
                let full = if named.is_absolute() { named.to_path_buf() } else { base.map_or_else(|| named.to_path_buf(), |b| b.join(named)) };
                if !self.open_path(&full) {
                    return;
                }
                self.active
            }
        };
        self.active = index;
        self.pending_focus = true;
        let doc = &mut self.docs[index];
        if !doc.hex_mode {
            let line = line.saturating_sub(1).min(doc.text.line_count().saturating_sub(1));
            doc.text.move_caret(line, 0, false);
        }
    }

    pub(super) fn open_assembler_box(&mut self) {
        let cpu = self.doc_cpu(self.active).unwrap_or(Cpu::Z80);
        let found = assemblers::installed(cpu);
        let chosen = self.assembler_for(cpu);
        self.dialog = Dialog::Assembler(Box::new(AssemblerBox { cpu, found, chosen }));
    }

    pub(super) fn assembler_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::Assembler(state) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut keep = false;
        let mut browse = false;
        egui::Window::new("Assembler").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(format!("{} source is assembled, when it's run (F9), with:", state.cpu.name()));
            ui.add_space(6.0);
            ui.radio_value(&mut state.chosen, Choice::Own, "Limoniter's own assembler (nothing to install)");
            for (found, path) in &state.found {
                let choice = Choice::Program { id: found.id, path: path.clone() };
                ui.radio_value(&mut state.chosen, choice, format!("{}  ({})", found.name, path.display()));
            }
            if let Choice::Program { id, path } = state.chosen.clone()
                && !state.found.iter().any(|(f, p)| f.id == id && *p == path)
            {
                let label = format!("{}  ({})", assemblers::external(id).map_or(id, |e| e.name), path.display());
                let choice = state.chosen.clone();
                ui.radio_value(&mut state.chosen, choice, label);
            }
            ui.add_space(4.0);
            let others: Vec<&assemblers::External> = assemblers::KNOWN.iter().filter(|e| e.cpu == state.cpu && !state.found.iter().any(|(f, _)| f.id == e.id)).collect();
            if !others.is_empty() {
                let words: Vec<String> = others.iter().map(|e| format!("{} ({})", e.name, e.website)).collect();
                ui.add(egui::Label::new(RichText::new(format!("Not found on this computer: {}. Once installed, they appear here; or point to one.", words.join(", "))).color(theme::text_dim()).size(12.0)).wrap());
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                browse = ui.button("Point to an assembler\u{2026}").clicked();
                keep = ui.button("OK").clicked();
                close |= ui.button("Cancel").clicked();
            });
        });
        if browse
            && let Some(path) = rfd::FileDialog::new().set_title("The assembler program").pick_file()
        {
            let name = path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            let id = assemblers::KNOWN.iter().find(|e| e.programs.iter().any(|p| name.contains(p))).map_or("sjasmplus", |e| e.id);
            if let Dialog::Assembler(state) = &mut self.dialog {
                state.chosen = Choice::Program { id, path };
            }
        }
        if keep {
            if let Dialog::Assembler(state) = std::mem::replace(&mut self.dialog, Dialog::None) {
                self.set_assembler(state.cpu, &state.chosen);
            }
            self.pending_focus = true;
        } else if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }

    /// What a line of the document in front became, for the status bar: "$8000: 3E 02 (7 T-states)".
    pub(super) fn line_assembled(&self, doc_id: u64, line: usize) -> Option<String> {
        let cache = self.running.assembled.as_ref().filter(|c| c.doc_id == doc_id)?;
        let doc = self.docs.iter().find(|d| d.id == doc_id)?;
        // Only while the text is as it was assembled, or the lines may have moved.
        if doc.text.version() != cache.version {
            return None;
        }
        let address = cache.assembly.address_of(0, line)?;
        let placed = cache.assembly.lines.iter().find(|p| p.file == 0 && p.line == line && !p.bytes.is_empty())?;
        let shown: Vec<String> = placed.bytes.iter().take(8).map(|b| format!("{b:02X}")).collect();
        let more = if placed.bytes.len() > 8 { format!(" \u{2026} ({} bytes)", placed.bytes.len()) } else { String::new() };
        let cpu = self.doc_cpu(self.docs.iter().position(|d| d.id == doc_id)?)?;
        let timing = cpu.isa().cycles(&placed.bytes).filter(|_| placed.bytes.len() <= 4).map(|t| format!(", {t} T-states")).unwrap_or_default();
        Some(format!("${address:04X}: {}{more}{timing}", shown.join(" ")))
    }
}

/// What a program assembled runs as: a tape for a Spectrum, or a .nex for the Next if the source says DEVICE
/// ZXSPECTRUMNEXT.
pub fn runnable_from(cpu: Cpu, assembled: &Assembled, stem: &str) -> Result<Runnable, String> {
    if let Some((format, bytes)) = &assembled.tape {
        let name = format!("{stem}.{}", format.ending());
        return Runnable::from_file(&name, bytes.clone()).ok_or_else(|| format!("{name} isn't a program Limoniter knows how to run."));
    }
    let assembly = &assembled.assembly;
    if cpu == Cpu::M6502 {
        // A Commodore 64's, unless it loads where another Commodore's BASIC starts.
        let bytes = crate::asm::output::commodore_program(assembly)?;
        return Runnable::from_file(&format!("{stem}.prg"), bytes).ok_or_else(|| "The program couldn't be made.".to_string());
    }
    let device = assembly.device.clone().unwrap_or_default();
    if device.contains("NEXT") {
        let bytes = crate::asm::output::next_file(assembly, None)?;
        return Ok(Runnable { machine: Machine::Next, format: Format::Nex, name: format!("{stem}.nex"), bytes: Arc::new(bytes) });
    }
    let machine = if device.contains("128") { Machine::Spectrum128 } else { Machine::Spectrum48 };
    let bytes = crate::asm::output::spectrum_tape(assembly, stem)?;
    Ok(Runnable { machine, format: Format::Tap, name: format!("{stem}.tap"), bytes: Arc::new(bytes) })
}
