//! The window's side of assembling: source in front is assembled when it's run (F9), with Limoniter's own
//! assembler or the one chosen; problems go in the Output panel, where clicking one goes to it, and are marked
//! beside their lines. The program assembled is kept, so the debugger knows which line is at which address.

use super::*;
use crate::asm::Assembly;
use crate::asm::output::Micro6502;
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

/// How the settings remember where an assembler a project names is on this computer: "where:gnu-as".
const WHERE: &str = "where:";

/// Whether a choice is GNU as, which reads only its own way of writing 68000 source.
fn row_is_gnu(choice: &Choice) -> bool {
    matches!(choice, Choice::Program { id, .. } if *id == assemblers::GNU_AS)
}

/// The box choosing an assembler for each processor.
pub(super) struct AssemblerBox {
    /// Each processor, the assemblers found for it, and the one chosen.
    pub rows: Vec<AssemblerRow>,
    /// The processor being chosen for.
    pub showing: usize,
    /// KickAssembler being downloaded, when asked, and what came of the last try.
    pub kickass: Option<std::sync::mpsc::Receiver<Result<PathBuf, String>>>,
    pub kickass_said: String,
}

pub(super) struct AssemblerRow {
    pub cpu: Cpu,
    pub found: Vec<(&'static assemblers::External, PathBuf)>,
    pub chosen: Choice,
    /// Whether the choice is the project's own, kept in its file, rather than the settings'.
    pub for_project: bool,
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

    /// The assembler a processor's source is built with: the project's, when it says, or else the settings'.
    /// A project's names the assembler only; where it is on this computer comes from the settings, or from looking
    /// where it's usually installed. One that can't be found has no path, so it can't be run, and nothing else is
    /// used in its place.
    pub(super) fn assembler_for(&self, cpu: Cpu) -> Choice {
        let chosen = self.running.assemblers.iter().find(|(id, _)| id == cpu.id()).map_or(Choice::Own, |(_, setting)| Choice::from_setting(setting));
        let Some(id) = self.project.as_ref().and_then(|p| p.assembler(cpu.id())) else {
            return chosen;
        };
        if id == assemblers::OWN {
            return Choice::Own;
        }
        let Some(known) = assemblers::external(id) else {
            return Choice::Own;
        };
        let remembered = self.running.assemblers.iter().find(|(k, _)| k.strip_prefix(WHERE) == Some(known.id)).map(|(_, setting)| Choice::from_setting(setting));
        // Where it is: as the settings choose it for other source, or as they remember it for projects, or else
        // wherever it's usually installed.
        [Some(chosen), remembered]
            .into_iter()
            .flatten()
            .find(|c| matches!(c, Choice::Program { id, .. } if *id == known.id))
            .unwrap_or_else(|| Choice::Program { id: known.id, path: assemblers::installed(cpu).into_iter().find(|(e, _)| e.id == known.id).map(|(_, path)| path).unwrap_or_default() })
    }

    /// The assembler for the source in front: as chosen, unless it can't read the source's way of writing 68000
    /// source, which is said rather than built wrongly. GNU as's source, with nothing chosen for it (Limoniter's own,
    /// by the settings rather than a project), is built with GNU as when it's installed.
    pub(super) fn assembler_for_source(&self, cpu: Cpu, language: &str) -> Result<Choice, String> {
        let gnu = assemblers::is_gnu_source(language);
        let mut choice = self.assembler_for(cpu);
        let project_says = self.project.as_ref().and_then(|p| p.assembler(cpu.id())).is_some();
        if gnu
            && choice == Choice::Own
            && !project_says
            && let Some((found, path)) = assemblers::installed(cpu).into_iter().find(|(e, _)| e.id == assemblers::GNU_AS)
        {
            choice = Choice::Program { id: found.id, path };
        }
        if let Some(problem) = assemblers::wrong_dialect(&choice, gnu) {
            return Err(problem);
        }
        if let Choice::Program { id, path } = &choice
            && path.as_os_str().is_empty()
        {
            let name = assemblers::external(id).map_or(*id, |e| e.name);
            return Err(crate::i18n::tf!("This project's {} source is built with {name}, which wasn't found on this computer. Install it, or point to it in File, Run on, Assemblers.", cpu.name()));
        }
        Ok(choice)
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
        let cpu = self.doc_cpu(self.active);
        // Spectrum BASIC has breakpoints too, by its line numbers, on the built-in Spectrum.
        let basic = cpu.is_none() && self.is_spectrum_basic(self.active) && matches!(self.docs[self.active].listing_of, Some((basic::Machine::Spectrum, _)) | None);
        // Files a debugger can run have breakpoints as well.
        // Its language worked out first: a file just opened hasn't been coloured yet.
        let debugged = cpu.is_none() && !basic && {
            self.docs[self.active].update_syntax();
            super::dap_host::debuggable(&self.docs[self.active].language_name())
        };
        let doc = &mut self.docs[self.active];
        match (cpu.is_some() || basic || debugged, doc.text.code.is_some()) {
            (true, false) => doc.text.code = Some(Box::default()),
            (false, true) => doc.text.code = None,
            _ => {}
        }
        self.intel_tick(cpu);
    }

    /// Keeps what's known about the code in front up to date: for assembler source, it's assembled quietly a
    /// moment after typing stops, and with Limoniter's own assembler chosen, its problems are marked as you go.
    fn intel_tick(&mut self, cpu: Option<Cpu>) {
        let index = self.active;
        let language = self.docs[index].language_name();
        let doc = &mut self.docs[index];
        if doc.hex_mode || doc.text.is_disk_backed() {
            doc.text.intel = None;
            return;
        }
        let Some(cpu) = cpu else {
            // Boriel's ZX Basic: its words and its library's, each described.
            if language == highlight::BASIC_BORIEL {
                if doc.text.intel.as_ref().is_none_or(|i| !i.boriel) {
                    doc.text.intel = Some(Box::new(crate::code_intel::CodeIntel { boriel: true, ..crate::code_intel::CodeIntel::keywords(crate::boriel::keywords()) }));
                }
                return;
            }
            // BASIC: its keywords.
            let spectrum = match &doc.listing_of {
                Some((basic::Machine::Next, _)) => Some(Some(basic::ZxDialect::Next)),
                _ if language == highlight::BASIC_SINCLAIR => Some(Some(basic::ZxDialect::Spectrum)),
                _ if language == highlight::BASIC_COMMODORE => Some(None),
                _ => None,
            };
            // C for the Amiga: its system functions, when the NDK is there.
            let amiga_c = spectrum.is_none() && amiga_c_source(doc);
            match spectrum {
                Some(dialect) if doc.text.intel.as_ref().is_none_or(|i| i.cpu.is_some() || i.words.is_empty() || i.boriel) => doc.text.intel = Some(Box::new(crate::code_intel::CodeIntel::keywords(basic::keywords(dialect)))),
                Some(_) => {}
                None if amiga_c => match crate::amiga_ndk::current() {
                    Some(ndk) => doc.text.intel.get_or_insert_with(Box::default).amiga = Some(ndk),
                    None if doc.text.intel.as_ref().is_some_and(|i| i.lsp || !i.build_problems.is_empty()) => {}
                    None => doc.text.intel = None,
                },
                // What a language server says is kept, and Markdown's own problems.
                None if doc.text.intel.as_ref().is_some_and(|i| i.lsp || !i.build_problems.is_empty()) || doc.text.markdown => {}
                None => doc.text.intel = None,
            }
            return;
        };
        if doc.text.intel.as_ref().is_none_or(|i| i.cpu != Some(cpu)) {
            doc.text.intel = Some(Box::new(crate::code_intel::CodeIntel::for_cpu(cpu)));
            self.intel_wait = None;
        }
        // 68000 source may be for the Amiga: its system functions, when the NDK is there.
        if cpu == Cpu::M68k
            && let Some(intel) = doc.text.intel.as_mut()
        {
            intel.amiga = crate::amiga_ndk::current();
        }
        // Limoniter's own assembler reads Motorola's way of writing 68000 source, so GNU as's would only give it
        // problems that aren't there: its hints are the NDK's alone.
        if assemblers::is_gnu_source(&language) {
            if let Some(code) = doc.text.code.as_mut() {
                code.problems.clear();
            }
            return;
        }
        let version = self.docs[index].text.version();
        let doc_id = self.docs[index].id;
        if self.docs[index].text.intel.as_ref().is_some_and(|i| i.assembly.is_some() && i.version == version) {
            return;
        }
        // A moment after the last change, so typing isn't held up.
        let now = std::time::Instant::now();
        match self.intel_wait {
            Some((id, v, since)) if id == doc_id && v == version => {
                if now.duration_since(since) < Duration::from_millis(300) {
                    return;
                }
            }
            _ => {
                let first = self.docs[index].text.intel.as_ref().is_some_and(|i| i.assembly.is_none());
                self.intel_wait = Some((doc_id, version, now));
                if !first {
                    return;
                }
            }
        }
        let doc = &self.docs[index];
        let path = std::path::absolute(doc.path_or_name()).unwrap_or_else(|_| doc.path_or_name());
        let text = doc.text.all_lines().iter().map(|l| l.as_ref()).collect::<Vec<&str>>().join("\n");
        let assembly = assemblers::assemble_own(cpu, &path, &text).assembly;
        let own = self.assembler_for(cpu) == Choice::Own;
        let doc = &mut self.docs[index];
        if own && let Some(code) = doc.text.code.as_mut() {
            code.problems = assembly.problems.iter().filter(|p| p.file == 0).map(|p| (p.line, p.message.clone())).collect();
        }
        if let Some(intel) = doc.text.intel.as_mut() {
            intel.hardware = (cpu == Cpu::M68k).then(|| Arc::new(crate::amiga_source::Source::scan(&text, Some(&assembly))));
            intel.atari = (cpu == Cpu::M68k).then(|| Arc::new(crate::st_source::Source::scan(&text, Some(&assembly))));
            intel.c64 = (cpu == Cpu::M6502).then(|| Arc::new(crate::c64_source::Source::scan(&text, Some(&assembly))));
            intel.assembly = Some(Arc::new(assembly));
            intel.version = version;
            intel.file = 0;
        }
    }

    /// The program the source in front assembles to, ready to run. With `report`, it's assembled whatever it takes
    /// (saving first for another program to read) and the problems are shown; without, only Limoniter's own
    /// assembler is used, quietly, to say what the program is for. An empty error means the problems are shown.
    pub(super) fn assemble_to_run(&mut self, cpu: Cpu, report: bool) -> Result<Runnable, String> {
        self.assemble_source(cpu, report, report)
    }

    /// Assembles the source in front, reporting what came of it with `report`, and with `save`, saving the file
    /// a processor's programs are saved as rather than run (a 6809's, or a 68000's with an ORG).
    fn assemble_source(&mut self, cpu: Cpu, report: bool, save: bool) -> Result<Runnable, String> {
        let choice = self.assembler_for_source(cpu, &self.docs[self.active].language_name())?;
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
                assemblers::assemble_own_to_run(cpu, &path, &text)
            }
            Choice::Program { id, path: program } if report => {
                let found = assemblers::external(id).ok_or(crate::i18n::tr("That assembler isn't known."))?;
                if doc.path.is_none() {
                    return Err(crate::i18n::tf!("Save the file first, so {} can read it.", found.name));
                }
                if doc.is_modified() {
                    let index = self.active;
                    if !self.save(index) {
                        return Err(crate::i18n::tr("The file couldn't be saved, so it wasn't assembled.").into());
                    }
                    if self.docs[index].saving.is_some() {
                        return Err(crate::i18n::tr("The file is being saved. Run it again once that's finished.").into());
                    }
                }
                let version = self.docs[self.active].text.version();
                let assembled = assemblers::assemble_external(found, program, &path)?;
                return self.keep_assembled(doc_id, version, &choice, assembled, &stem, report, save);
            }
            Choice::Program { .. } => return Err(crate::i18n::tr("Not assembled yet.").into()),
        };
        self.keep_assembled(doc_id, version, &choice, assembled, &stem, report, save)
    }

    #[allow(clippy::too_many_arguments)]
    fn keep_assembled(&mut self, doc_id: u64, version: u64, choice: &Choice, assembled: Assembled, stem: &str, report: bool, save: bool) -> Result<Runnable, String> {
        let cpu = self.doc_cpu(self.active).unwrap_or(Cpu::Z80);
        // For 6502 source: the machine a `; machine:` line names, and the one chosen when F9 asked.
        let micro = match self.docs.iter().find(|d| d.id == doc_id) {
            Some(doc) if cpu == Cpu::M6502 => {
                let lines = doc.text.all_lines();
                Micro { said: Micro6502::said_in(lines.iter().map(|l| l.as_ref())), asked: doc.machine_6502 }
            }
            _ => Micro::default(),
        };
        let made = if assembled.assembly.ok() { made_from(cpu, &assembled, stem, micro) } else { Err(String::new()) };
        // An Amiga program is crunched beside the source too, if Run on, Amiga crunchers and music says to.
        match &made {
            Ok(Made::Run(run)) if report => {
                self.crunch_after_build(doc_id, &run.name, &run.bytes);
                self.icon_after_build(doc_id, &run.name, &run.bytes, false);
                self.c64_pack_after_build(doc_id, &run.name, &run.bytes);
            }
            Ok(Made::Saved { name, bytes }) if report => {
                self.crunch_after_build(doc_id, name, bytes);
                self.icon_after_build(doc_id, name, bytes, false);
                self.c64_pack_after_build(doc_id, name, bytes);
            }
            _ => {}
        }
        // A file to save is saved (only when asked to assemble, not for a menu), and nothing runs.
        let saved = match &made {
            Ok(Made::Saved { name, bytes }) if save => {
                let folder = self.docs.iter().find(|d| d.id == doc_id).and_then(|d| d.path.as_deref()).and_then(Path::parent).map(Path::to_path_buf);
                Some(match folder {
                    Some(folder) => {
                        let path = folder.join(name);
                        match crate::codec::write_file(&path, bytes) {
                            Ok(()) => crate::i18n::tf!("Saved {} ({} bytes), to load into an emulator.", path.display(), bytes.len()),
                            Err(e) => crate::i18n::tf!("{} couldn't be saved: {e}", path.display()),
                        }
                    }
                    None => crate::i18n::tf!("Save the source first, so {name} can be saved beside it."),
                })
            }
            _ => None,
        };
        // The files the source asks for (SAVENEX, SAVEBIN and the rest) are written beside it, as sjasmplus would.
        let written = if report && assembled.assembly.ok() && !assembled.assembly.outputs.is_empty() {
            let folder = self.docs.iter().find(|d| d.id == doc_id).and_then(|d| d.path.as_deref()).and_then(Path::parent).map(Path::to_path_buf);
            match folder {
                Some(folder) => assembled
                    .assembly
                    .outputs
                    .iter()
                    .map(|output| {
                        let path = folder.join(&output.name);
                        let result = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| crate::codec::write_file(&path, &output.bytes));
                        match result {
                            Ok(()) => crate::i18n::tf!("Wrote {} ({} bytes).", path.display(), output.bytes.len()),
                            Err(e) => crate::i18n::tf!("{} couldn't be written: {e}", path.display()),
                        }
                    })
                    .collect(),
                None => vec![crate::i18n::tr("Save the source first, so the files it asks for can be written beside it.").to_string()],
            }
        } else {
            Vec::new()
        };
        let runnable = match made {
            Ok(Made::Run(run)) => Ok(run),
            Ok(Made::Saved { .. }) => Err(String::new()),
            Err(problem) => Err(problem),
        };
        if report {
            self.report_assembly(doc_id, choice, &assembled, &runnable);
            if let Some(words) = saved {
                self.script_print(words, false);
            }
            for words in written {
                self.script_print(words, false);
            }
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
        let mut lines = vec![crate::i18n::tf!("Assembling {name} with {}", choice.name())];
        self.memory_map = None;
        if !assembly.ok() {
            // What the other assembler printed, as it printed it, besides the problems read from it.
            lines.extend(assembled.printed.iter().filter(|l| assemblers::problem_line(l).is_none() && !l.trim().is_empty()).cloned());
        }
        for problem in &assembly.problems {
            let file = assembly.files.get(problem.file).map(|f| f.display().to_string()).unwrap_or_else(|| name.clone());
            lines.push(format!("{file}({}): {}", problem.line + 1, problem.message));
        }
        for warning in &assembly.warnings {
            let file = assembly.files.get(warning.file).map(|f| f.display().to_string()).unwrap_or_else(|| name.clone());
            lines.push(format!("{file}({}): warning: {}", warning.line + 1, warning.message));
        }
        lines.extend(assembly.notes.iter().map(|n| format!("> {n}")));
        match runnable {
            Ok(run) => {
                let (low, high) = (assembly.lowest().unwrap_or(0), assembly.highest().unwrap_or(0));
                // What was made, by the kind of file it is: each a whole sentence, so it translates as one.
                // A program made whole (an Amiga program, or another assembler's .prg) is named by the kind of
                // file it is, each a whole sentence so it translates as one; otherwise its bytes are given.
                let size = match run.format.medium() {
                    _ if assembled.tape.is_none() => crate::i18n::tf!("{} bytes from ${low:04X} to ${:04X}, starting at ${:04X}", high - low, (high - 1).max(low), assembly.start().unwrap_or(low)),
                    Some("tape") => crate::i18n::tr("A tape was made").to_string(),
                    Some("disk") => crate::i18n::tr("A disk image was made").to_string(),
                    Some("cartridge") => crate::i18n::tr("A cartridge was made").to_string(),
                    _ => crate::i18n::tr("A program was made").to_string(),
                };
                lines.push(format!("{size}: {} for the {}.", run.name, run.machine.name()));
                let chunks: Vec<(i64, usize)> = assembly.chunks.iter().map(|c| (c.address, c.bytes.len())).collect();
                if let Some(map) = crate::memory_map::map(run.machine, &chunks) {
                    lines.push(map.summary());
                    lines.extend(map.warnings.iter().map(|w| format!("Memory: {w}")));
                    self.memory_map = Some(map);
                }
            }
            Err(problem) if problem == ASK_6502 => lines.push(crate::i18n::tr("It doesn't say whether it's for a Commodore 64, a BBC Micro, an Atari 2600 or an NES. Put ; machine: c64, ; machine: bbc, ; machine: 2600 or ; machine: nes near the top to say.").into()),
            Err(problem) if !problem.is_empty() => lines.push(problem.clone()),
            Err(_) if assembly.ok() => {
                let (low, high) = (assembly.lowest().unwrap_or(0), assembly.highest().unwrap_or(0));
                lines.push(crate::i18n::tf!("{} bytes from ${low:04X} to ${:04X}, starting at ${:04X}.", high - low, (high - 1).max(low), assembly.start().unwrap_or(low)));
            }
            Err(_) => lines.push(match assembly.problems.len() {
                1 => crate::i18n::tr("1 problem.").to_string(),
                errors => crate::i18n::tf!("{errors} problems."),
            }),
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

    /// Assembles the source in front and saves the program beside it (a .tap, .prg, .nex or .bin), without running
    /// it.
    pub(super) fn assemble_and_save(&mut self) {
        let Some(cpu) = self.doc_cpu(self.active) else {
            return self.message("Assemble and save", crate::i18n::tr("This isn't assembler source Limoniter knows: Z80, 6502, 6809 or 68000.").into());
        };
        let Some(folder) = self.docs[self.active].path.as_deref().and_then(Path::parent).map(Path::to_path_buf) else {
            return self.message("Assemble and save", crate::i18n::tr("Save the source first, so the program can be saved beside it.").into());
        };
        // A 6809 or 68000 program is saved by assembling it; the others are made ready to run, and saved here.
        if let Ok(run) = self.assemble_to_run(cpu, true) {
            let path = folder.join(&run.name);
            let words = match crate::codec::write_file(&path, &run.bytes) {
                Ok(()) => crate::i18n::tf!("Saved {} ({} bytes).", path.display(), run.bytes.len()),
                Err(e) => crate::i18n::tf!("{} couldn't be saved: {e}", path.display()),
            };
            self.script_print(words, false);
        }
    }

    /// Assembles the source in front and saves just the bytes it makes, from the lowest address to the highest,
    /// with no loader or header: a file to INCBIN, LOAD "" CODE at its address, or put in a ROM.
    pub(super) fn assemble_binary(&mut self) {
        let (bytes, low, start) = match self.assembled_bytes() {
            Ok(made) => made,
            Err(Some(problem)) => return self.message("Assemble and save as binary", problem),
            Err(None) => return,
        };
        let doc = &self.docs[self.active];
        let stem = doc.path_or_name().file_stem().map_or_else(|| "program".to_string(), |s| s.to_string_lossy().into_owned());
        let mut picker = rfd::FileDialog::new().set_title(crate::i18n::tr("Save the assembled bytes as")).set_file_name(format!("{stem}.bin")).add_filter(crate::i18n::tr("Binary"), &["bin"]).add_filter(crate::i18n::tr("Any file"), &["*"]);
        if let Some(folder) = doc.path.as_deref().and_then(Path::parent) {
            picker = picker.set_directory(folder);
        }
        let Some(path) = picker.save_file() else {
            return;
        };
        let words = match crate::codec::write_file(&path, &bytes) {
            Ok(()) => {
                let run = if start == low { String::new() } else { format!(", starting at {start} (${start:04X})") };
                crate::i18n::tf!("Saved {} bytes to {}: load them at {low} (${low:04X}){run}.", bytes.len(), path.display())
            }
            Err(e) => crate::i18n::tf!("{} couldn't be saved: {e}", path.display()),
        };
        self.script_print(words, false);
    }

    /// The source in front, assembled: its bytes from the lowest address to the highest, that address, and where
    /// it starts. Problems are in the Output panel, so they give no message here (None).
    pub(super) fn assembled_bytes(&mut self) -> Result<(Vec<u8>, i64, i64), Option<String>> {
        let Some(cpu) = self.doc_cpu(self.active) else {
            return Err(Some(crate::i18n::tr("This isn't assembler source Limoniter knows: Z80, 6502, 6809 or 68000.").into()));
        };
        let doc_id = self.docs[self.active].id;
        // What can't run (a .nex the source doesn't set up, say) can still be saved as bytes.
        let _ = self.assemble_source(cpu, true, false);
        let assembly = self.running.assembled.as_ref().filter(|cache| cache.doc_id == doc_id).map(|cache| cache.assembly.clone()).ok_or(None)?;
        if !assembly.ok() {
            return Err(None);
        }
        let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
            return Err(Some(crate::i18n::tf!("{} gave back no bytes to save, only its own file. Choose Limoniter's own assembler, or sjasmplus, in File, Run on, Assemblers, to save them.", self.assembler_for(cpu).name())));
        };
        Ok((assembly.image(low, high), low, assembly.start().unwrap_or(low)))
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

    /// Opens the box choosing assemblers, on the processor of the source in front, if it's assembler source.
    pub(super) fn open_assembler_box(&mut self) {
        let showing = self.doc_cpu(self.active).and_then(|cpu| Cpu::ALL.iter().position(|c| *c == cpu)).unwrap_or(0);
        let rows = Cpu::ALL
            .iter()
            .map(|cpu| AssemblerRow { cpu: *cpu, found: assemblers::installed(*cpu), chosen: self.assembler_for(*cpu), for_project: self.project.as_ref().is_some_and(|p| p.assembler(cpu.id()).is_some()) })
            .collect();
        self.dialog = Dialog::Assembler(Box::new(AssemblerBox { rows, showing, kickass: None, kickass_said: String::new() }));
    }

    pub(super) fn assembler_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::Assembler(state) = &mut self.dialog else {
            return false;
        };
        // A download of KickAssembler that's finished: chosen for 6502 source.
        if let Some(rx) = &state.kickass {
            match rx.try_recv() {
                Ok(Ok(path)) => {
                    state.kickass = None;
                    state.kickass_said = crate::i18n::tf!("Downloaded to {}.", path.display());
                    if let Some(row) = state.rows.iter_mut().find(|r| r.cpu == Cpu::M6502) {
                        row.found = assemblers::installed(Cpu::M6502);
                        row.chosen = Choice::Program { id: "kickass", path };
                    }
                }
                Ok(Err(problem)) => {
                    state.kickass = None;
                    state.kickass_said = problem;
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            }
        }
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut keep = false;
        let mut browse = false;
        let mut fetch_kickass = false;
        let project_name = self.project.as_ref().map(|p| p.name.clone());
        crate::fit::Dialog::new(crate::i18n::tr("Assemblers")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show_with_buttons(ctx, |ui, part| match part {
            crate::fit::Part::Body => {
                ui.label(crate::i18n::tr("Source for each processor is assembled, when it's run (F9) or saved, with the assembler chosen for it."));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    for (index, row) in state.rows.iter().enumerate() {
                        let own = if row.chosen == Choice::Own { crate::i18n::tr("Limoniter's own").to_string() } else { assemblers::external(match &row.chosen { Choice::Program { id, .. } => id, Choice::Own => "" }).map_or_else(String::new, |e| e.name.to_string()) };
                        if ui.selectable_label(state.showing == index, crate::i18n::tr(row.cpu.name())).on_hover_text(own).clicked() {
                            state.showing = index;
                        }
                    }
                });
                ui.separator();
                let row = &mut state.rows[state.showing];
                ui.label(crate::i18n::tf!("{} source is assembled with:", row.cpu.name()));
                ui.add_space(4.0);
                ui.radio_value(&mut row.chosen, Choice::Own, crate::i18n::tr("Limoniter's own assembler (nothing to install)"));
                for (found, path) in &row.found {
                    let choice = Choice::Program { id: found.id, path: path.clone() };
                    ui.radio_value(&mut row.chosen, choice, crate::i18n::tf!("{}  ({})", found.name, path.display()));
                }
                if let Choice::Program { id, path } = row.chosen.clone()
                    && !row.found.iter().any(|(f, p)| f.id == id && *p == path)
                {
                    let label = format!("{}  ({})", assemblers::external(id).map_or(id, |e| e.name), path.display());
                    let choice = row.chosen.clone();
                    ui.radio_value(&mut row.chosen, choice, crate::i18n::tr(&label));
                }
                ui.add_space(4.0);
                let others: Vec<&assemblers::External> = assemblers::KNOWN.iter().filter(|e| e.cpu == row.cpu && !row.found.iter().any(|(f, _)| f.id == e.id)).collect();
                if !others.is_empty() {
                    let words: Vec<String> = others.iter().map(|e| format!("{} ({})", e.name, e.website)).collect();
                    ui.add(egui::Label::new(RichText::new(crate::i18n::tf!("Not found on this computer: {}. Once installed, they appear here; or point to one.", words.join(", "))).color(theme::text_dim()).size(12.0)).wrap());
                }
                // KickAssembler can be fetched from its author's website, on request, with a plain word about its terms.
                if row.cpu == Cpu::M6502 && others.iter().any(|e| e.id == "kickass") {
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(RichText::new(crate::c64_assemblers::KICKASS_NOTE).size(12.0)).wrap());
                    ui.horizontal(|ui| {
                        if state.kickass.is_some() {
                            ui.spinner();
                            ui.label(crate::i18n::tr("Downloading\u{2026}"));
                        } else {
                            fetch_kickass = ui.button(crate::i18n::tr("Download KickAssembler from theweb.dk")).on_hover_text(crate::c64_assemblers::KICKASS_DOWNLOAD).clicked();
                        }
                    });
                    if crate::c64_assemblers::find_java().is_none() {
                        ui.colored_label(theme::change_unsaved(), crate::i18n::tr("Java wasn't found on this computer; KickAssembler needs it to run."));
                    }
                }
                if !state.kickass_said.is_empty() {
                    ui.colored_label(theme::text_dim(), &state.kickass_said);
                }
                // A project can keep its own choice, so it's always built with the assembler its source is written for.
                if let Some(name) = &project_name {
                    ui.add_space(4.0);
                    let row = &mut state.rows[state.showing];
                    ui.checkbox(&mut row.for_project, crate::i18n::tf!("Only for the project {name}, kept in its file"))
                        .on_hover_text(crate::i18n::tr("The project's source is then always built with this assembler, wherever the project is opened, whatever is chosen for other source; if it isn't installed, nothing else is used instead."));
                }
                if row_is_gnu(&state.rows[state.showing].chosen) {
                    ui.add(egui::Label::new(RichText::new(crate::i18n::tr("GNU as reads only 68000 source written its own way (Assembly 680x0 (GNU as)), and the others only Motorola's: source is never given to one that can't read it.")).color(theme::text_dim()).size(12.0)).wrap());
                }
            }
            crate::fit::Part::Buttons => {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    browse = ui.button(crate::i18n::tr("Point to an assembler\u{2026}")).clicked();
                    keep = ui.button(crate::i18n::tr("OK")).clicked();
                    close |= ui.button(crate::i18n::tr("Cancel")).clicked();
                });
            }
        });
        if fetch_kickass
            && let Dialog::Assembler(state) = &mut self.dialog
        {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::c64_assemblers::download_kickass());
            });
            state.kickass_said.clear();
            state.kickass = Some(rx);
        }
        if browse
            && let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("The assembler program")).pick_file()
            && let Dialog::Assembler(state) = &mut self.dialog
        {
            let row = &mut state.rows[state.showing];
            match assemblers::recognise(row.cpu, &path) {
                Some(id) => row.chosen = Choice::Program { id, path },
                None => {
                    let known: Vec<&str> = assemblers::KNOWN.iter().filter(|e| e.cpu == row.cpu).map(|e| e.name).collect();
                    let cpu = row.cpu.name();
                    self.message("Assemblers", crate::i18n::tf!("Limoniter doesn't know that program, so it can't read what it prints or the program it makes. For {cpu} source it works with {}.", known.join(" and ")));
                    return true;
                }
            }
        }
        if keep {
            if let Dialog::Assembler(state) = std::mem::replace(&mut self.dialog, Dialog::None) {
                let mut project_changed = false;
                for row in &state.rows {
                    let id = match &row.chosen {
                        Choice::Own => assemblers::OWN,
                        Choice::Program { id, .. } => id,
                    };
                    let wanted = row.for_project.then_some(id);
                    if let Some(project) = self.project.as_mut()
                        && project.assembler(row.cpu.id()) != wanted
                    {
                        project.set_assembler(row.cpu.id(), wanted);
                        project_changed = true;
                    }
                    if !row.for_project {
                        self.set_assembler(row.cpu, &row.chosen);
                    } else if let Choice::Program { id, .. } = &row.chosen {
                        // The project's choice leaves the settings' for other source as it was, but where the program
                        // is on this computer is remembered for it.
                        let key = format!("{WHERE}{id}");
                        self.running.assemblers.retain(|(k, _)| *k != key);
                        self.running.assemblers.push((key, row.chosen.setting()));
                        self.save_settings_if_changed();
                    }
                }
                if project_changed {
                    self.save_project();
                }
                self.running.assembled = None;
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
        let unit = if cpu == Cpu::Z80 { "T-states" } else { "cycles" };
        let timing = if cpu == Cpu::M6809 {
            // The 6309's native mode is told from the source, so the whole program is looked at.
            crate::m6809_timing::line(&cache.assembly, 0, line).map(|(_, mode, t)| format!(", {}", crate::m6809_timing::words(t, mode))).unwrap_or_default()
        } else if cpu == Cpu::M68k {
            // So is the 680x0 it's for (MACHINE 68008), and only the 68000's and 68008's times are given.
            crate::m68k_timing::line(&cache.assembly, 0, line).map(|(_, chosen, t)| format!(", {}", crate::m68k_timing::words(t, chosen))).unwrap_or_default()
        } else {
            cpu.isa().cycles_at(&placed.bytes, address).filter(|_| placed.bytes.len() <= 4).map(|t| format!(", {t} {unit}")).unwrap_or_default()
        };
        Some(format!("${address:04X}: {}{more}{timing}", shown.join(" ")))
    }
}

/// What a program assembled runs as: a tape for a Spectrum, or a .nex for the Next if the source says DEVICE
/// ZXSPECTRUMNEXT.
/// What a program assembled is made into: something to run, or, for a processor nothing here runs yet, a file to
/// save beside its source.
pub enum Made {
    Run(Runnable),
    Saved { name: String, bytes: Vec<u8> },
}

/// What runnable_from gives back for 6502 code when it can't tell which machine it's for, and nothing says: F9
/// asks.
pub const ASK_6502: &str = "\u{1}which 6502 machine";

/// The machine 6502 source is for, as it says and as chosen when asked. What the source says comes first, then
/// what the code itself shows (see `machine_6502`), then what was chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Micro {
    pub said: Option<Micro6502>,
    pub asked: Option<Micro6502>,
}

pub fn made_from(cpu: Cpu, assembled: &Assembled, stem: &str, micro: Micro) -> Result<Made, String> {
    match cpu {
        // A Vectrex cartridge; a Dragon DOS binary, or a disk with it on; or a Color Computer binary, which LOADM
        // loads, and XRoar runs (and debugs).
        Cpu::M6809 => {
            let said = assembled.source.as_deref().and_then(|text| machine_6809(text.lines()));
            let assembly = &assembled.assembly;
            let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
                return Err(crate::i18n::tr("Nothing was assembled.").into());
            };
            let image = assembly.image(low, high);
            if said == Some(Machine::Vectrex) || (said.is_none() && low == 0 && image.starts_with(b"g GCE")) {
                return vectrex_cartridge(low, image, stem).map(Made::Run);
            }
            match said {
                Some(machine @ (Machine::Dragon32 | Machine::Dragon64)) => {
                    if image.len() > 0xFFFF {
                        return Err(crate::i18n::tr("A Dragon DOS binary holds at most 65,535 bytes.").into());
                    }
                    let start = assembly.start().unwrap_or(low);
                    let bytes = crate::dragon_dos::binary(low as u16, start as u16, &image);
                    // On a Dragon DOS disk, for DragonDOS to load and run, if the source or the setting says so.
                    if crate::dragon_run::wants_disk(assembled.source.as_deref().unwrap_or_default().lines()) {
                        let (name, disk) = crate::dragon_run::disk_holding(stem, &bytes)?;
                        return Ok(Made::Run(Runnable { machine, format: Format::Vdk, name, bytes: Arc::new(disk) }));
                    }
                    Ok(Made::Run(Runnable { machine, format: Format::CocoBin, name: format!("{stem}.bin"), bytes: Arc::new(bytes) }))
                }
                _ => {
                    let bytes = crate::asm::output::coco_binary(assembly)?;
                    Ok(Made::Run(Runnable { machine: Machine::CoCo, format: Format::CocoBin, name: format!("{stem}.bin"), bytes: Arc::new(bytes) }))
                }
            }
        }
        // A cartridge for one of Sega's consoles, or the ColecoVision, as the source says or its header shows.
        Cpu::M68k | Cpu::Z80 if assembled.tape.is_none() && let Some(made) = console_cartridge(cpu, assembled, stem) => made.map(Made::Run),
        // An Amiga executable, made by the assembler from source for the Amiga.
        Cpu::M68k if assembled.tape.is_some() => runnable_from(cpu, assembled, stem, micro).map(Made::Run),
        // An Atari ST program, if it isn't put at an address of its own: else the bytes as they go in memory.
        Cpu::M68k if !assembled.assembly.absolute && assembled.tape.is_none() && assembled.assembly.lowest() == Some(0) => {
            let shift = 0x10000;
            let path = assembled.assembly.files.first().cloned().unwrap_or_default();
            let source = assembled.source.clone().or_else(|| std::fs::read_to_string(&path).ok()).ok_or(crate::i18n::tr("The source couldn't be read again to make the program movable."))?;
            let moved = crate::assemblers::assemble_own_at(Cpu::M68k, &path, &source, shift);
            let bytes = crate::asm::output::atari_program(&assembled.assembly, &moved, shift)?;
            Runnable::from_file(&format!("{stem}.tos"), bytes).map(Made::Run).ok_or_else(|| crate::i18n::tr("The Atari ST program couldn't be made.").into())
        }
        Cpu::M68k => {
            let assembly = &assembled.assembly;
            let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
                return Err(crate::i18n::tr("Nothing was assembled.").into());
            };
            Ok(Made::Saved { name: format!("{stem}.bin"), bytes: assembly.image(low, high) })
        }
        Cpu::X86 => pc_program(&assembled.assembly, stem).map(Made::Run),
        // The SNES's sound processor's code, to be loaded into its RAM by the SNES's own program.
        Cpu::Spc700 => {
            let assembly = &assembled.assembly;
            let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
                return Err(crate::i18n::tr("Nothing was assembled.").into());
            };
            Ok(Made::Saved { name: format!("{stem}.bin"), bytes: assembly.image(low, high) })
        }
        // A Game Boy cartridge, its header made complete.
        Cpu::Sm83 => {
            let bytes = crate::gameboy::cartridge(&assembled.assembly, stem)?;
            let format = if crate::gameboy::ending(&bytes) == "gbc" { Format::Gbc } else { Format::Gb };
            Ok(Made::Run(Runnable { machine: Machine::GameBoy, format, name: format!("{stem}.{}", format.ending()), bytes: Arc::new(bytes) }))
        }
        // A RISC OS application's code, run where RISC OS puts it, at &8000: an Absolute file (type FF8), named
        // as RISC OS's files are kept on other computers, with ,ff8 after the name.
        Cpu::Arm => {
            let assembly = &assembled.assembly;
            let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
                return Err(crate::i18n::tr("Nothing was assembled.").into());
            };
            let name = if low == 0x8000 { format!("{stem},ff8") } else { format!("{stem}.bin") };
            Ok(Made::Saved { name, bytes: assembly.image(low, high) })
        }
        _ => runnable_from(cpu, assembled, stem, micro).map(Made::Run),
    }
}

/// A Mega Drive cartridge from 68000 source, or a Master System, Game Gear, SG-1000 or ColecoVision cartridge
/// from Z80 source, if the source says so (; machine: megadrive, sms, gamegear, sg1000 or coleco) or its bytes
/// show it: a Mega Drive's header at $100, TMR SEGA at $7FF0, or a ColecoVision's AA 55 at $8000. None for
/// anything else, which is made as before.
fn console_cartridge(cpu: Cpu, assembled: &Assembled, stem: &str) -> Option<Result<Runnable, String>> {
    let assembly = &assembled.assembly;
    let lines = || assembled.source.as_deref().unwrap_or_default().lines();
    let (low, high) = (assembly.lowest()?, assembly.highest()?);
    let image = || assembly.image(low, high);
    let at_zero = |what: &str| if low == 0 { Ok(()) } else { Err(crate::i18n::tf!("A {what} cartridge starts at address 0, but this starts at ${low:04X}: put ORG 0 at its top.")) };
    let (machine, format, bytes) = if cpu == Cpu::M68k {
        if !crate::sega::says_mega_drive(lines()) && !(low == 0 && crate::sega::is_mega_drive(&image())) {
            return None;
        }
        let made = at_zero("Mega Drive").and_then(|()| crate::sega::fix_mega_drive(image()));
        (Machine::MegaDrive, Format::MdRom, made)
    } else {
        let said = crate::sega::machine_z80(lines());
        let shown = || {
            let bytes = image();
            if low == 0x8000 && crate::sega::is_coleco(&bytes) {
                Some(Machine::ColecoVision)
            } else if low == 0 {
                crate::sega::sms_header(&bytes).map(|(_, region)| if region >= 5 { Machine::GameGear } else { Machine::MasterSystem })
            } else {
                None
            }
        };
        match said.or_else(shown)? {
            Machine::MasterSystem => (Machine::MasterSystem, Format::SmsRom, at_zero("Master System").and_then(|()| crate::sega::fix_sms(image(), false))),
            Machine::GameGear => (Machine::GameGear, Format::GgRom, at_zero("Game Gear").and_then(|()| crate::sega::fix_sms(image(), true))),
            Machine::Sg1000 => (Machine::Sg1000, Format::SgRom, at_zero("SG-1000").and_then(|()| crate::sega::fix_sg1000(image()))),
            _ if low != 0x8000 => (Machine::ColecoVision, Format::ColRom, Err(crate::i18n::tf!("A ColecoVision cartridge starts at $8000, but this starts at ${low:04X}: put ORG $8000 at its top."))),
            _ => (Machine::ColecoVision, Format::ColRom, crate::sega::fix_coleco(image())),
        }
    };
    Some(bytes.map(|bytes| Runnable { machine, format, name: format!("{stem}.{}", format.ending()), bytes: Arc::new(bytes) }))
}

/// Which machine 6809 source says it's for, near its top: ; machine: dragon32 (or dragon64, coco or vectrex).
pub fn machine_6809<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<Machine> {
    lines.into_iter().take(50).find_map(|line| {
        let line = line.trim_start();
        let comment = line.strip_prefix(';').or_else(|| line.strip_prefix('*')).or_else(|| line.strip_prefix("//"))?;
        let comment = comment.trim_start_matches([';', '/', '*', ' ', '\t']).to_ascii_lowercase();
        let value = comment.strip_prefix("machine")?.trim_start().strip_prefix(':')?.trim().replace(' ', "");
        match value.as_str() {
            "dragon" | "dragon32" => Some(Machine::Dragon32),
            "dragon64" => Some(Machine::Dragon64),
            "coco" | "colorcomputer" | "trs-80colorcomputer" => Some(Machine::CoCo),
            "vectrex" => Some(Machine::Vectrex),
            _ => None,
        }
    })
}

/// The machine Z80 source names in a `; machine:` comment near its top, where it's one with a way of its own to run
/// code: the ZX81.
pub fn machine_z80<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<Machine> {
    lines.into_iter().take(50).find_map(|line| {
        let comment = line.trim_start().strip_prefix(';').or_else(|| line.trim_start().strip_prefix("//"))?;
        let comment = comment.trim_start_matches([';', '/', ' ', '\t']).to_ascii_lowercase();
        let value = comment.strip_prefix("machine")?.trim_start().strip_prefix(':')?.trim().replace(' ', "");
        matches!(value.as_str(), "zx81" | "ts1000" | "timexsinclair1000").then_some(Machine::Zx81)
    })
}

/// A Vectrex cartridge: its bytes from address 0, which have to start with the copyright line the BIOS looks for.
pub fn vectrex_cartridge(low: i64, image: Vec<u8>, stem: &str) -> Result<Runnable, String> {
    if low != 0 {
        return Err(crate::i18n::tf!("A Vectrex cartridge starts at address 0, but this starts at ${low:04X}: put ORG 0 at its top."));
    }
    if !image.starts_with(b"g GCE") {
        return Err(crate::i18n::tr("A Vectrex cartridge starts with its copyright line, FCC \"g GCE 1982\" and FCB $80, or the Vectrex won't run it.").into());
    }
    if image.len() > 0x8000 {
        return Err(crate::i18n::tf!("A Vectrex cartridge holds 32K, and this is {} bytes.", image.len()));
    }
    Ok(Runnable { machine: Machine::Vectrex, format: Format::VecCart, name: format!("{stem}.vec"), bytes: Arc::new(image) })
}

/// x86 code as the PC runs it: a boot sector (ORG 7C00h, or 512 bytes ending 55 AA) on a floppy that boots, else
/// a DOS .COM program, which DOS loads at 100h.
pub fn pc_program(assembly: &crate::asm::Assembly, stem: &str) -> Result<Runnable, String> {
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err(crate::i18n::tr("Nothing was assembled.").into());
    };
    let bytes = assembly.image(low, high);
    let boot = low == 0x7C00 || (bytes.len() == 512 && bytes[510..] == [0x55, 0xAA]);
    if boot {
        if bytes.len() > 512 {
            return Err(crate::i18n::tf!("The boot sector is {} bytes: it has 512, the last two 55 AA (end it with TIMES 510-($-$$) DB 0 and DW 0AA55h).", bytes.len()));
        }
        if bytes.len() < 512 || bytes[510..] != [0x55, 0xAA] {
            return Err(crate::i18n::tr("A boot sector ends with 55 AA at byte 510, or the PC won't boot it: end the source with TIMES 510-($-$$) DB 0 and DW 0AA55h.").into());
        }
        return Ok(Runnable { machine: Machine::Pc, format: Format::Img, name: format!("{stem}.img"), bytes: Arc::new(crate::running::pc_boot_floppy(&bytes)) });
    }
    if low != 0x100 {
        return Err(crate::i18n::tf!("A DOS .COM program is loaded at 100h, but this starts at {low:X}h: put ORG 100h at its top (or ORG 7C00h for a boot sector)."));
    }
    if bytes.len() > 0xFF00 {
        return Err(crate::i18n::tf!("A .COM program fits in 65,280 bytes, and this is {}.", bytes.len()));
    }
    Ok(Runnable { machine: Machine::Pc, format: Format::Com, name: format!("{stem}.com"), bytes: Arc::new(bytes) })
}

pub fn runnable_from(cpu: Cpu, assembled: &Assembled, stem: &str, micro: Micro) -> Result<Runnable, String> {
    if let Some((format, bytes)) = &assembled.tape {
        let name = format!("{stem}.{}", format.ending());
        return Runnable::from_file(&name, bytes.clone()).ok_or_else(|| crate::i18n::tf!("{name} isn't a program Limoniter knows how to run."));
    }
    let assembly = &assembled.assembly;
    // A program the source made itself (SAVENEX, SAVESNA, SAVETAP) is the one run, as it was meant to be.
    for ending in ["nex", "sna", "tap"] {
        if let Some(output) = assembly.outputs.iter().rev().find(|o| Path::new(&o.name).extension().is_some_and(|e| e.eq_ignore_ascii_case(ending)) && !o.bytes.is_empty()) {
            let name = Path::new(&output.name).file_name().map_or_else(|| output.name.clone(), |n| n.to_string_lossy().into_owned());
            if let Some(run) = Runnable::from_file(&name, output.bytes.clone()) {
                return Ok(run);
            }
        }
    }
    // A dot command (put at $2000, saved as a file with no ending) runs from a Next's SD card, not on its own.
    if let Some(dot) = assembly.outputs.iter().find(|o| !o.name.trim_end_matches('.').contains('.') || o.name.to_ascii_lowercase().contains("dot/"))
        && assembly.lowest().is_some_and(|low| (0x2000..0x4000).contains(&low))
    {
        let name = Path::new(&dot.name).file_name().map_or_else(|| dot.name.clone(), |n| n.to_string_lossy().into_owned());
        return Err(crate::i18n::tf!("This is a dot command, run by typing .{} on a Next: put {name} in the dot folder of the Next's SD card (open the card's image in Limoniter and add it, or copy it to the card).", name.trim_end_matches('.')));
    }
    if cpu == Cpu::M6502 {
        return match micro.said.or_else(|| crate::asm::output::machine_6502(assembly)).or(micro.asked) {
            // A BBC Micro's, on a disc that *RUNs it as it boots.
            Some(Micro6502::Bbc) => {
                let (load, exec, code) = crate::asm::output::bbc_program(assembly)?;
                let bytes = crate::running::bbc_code_disc(stem, &code, load, exec);
                Ok(Runnable { machine: Machine::Bbc, format: Format::Ssd, name: format!("{stem}.ssd"), bytes: Arc::new(bytes) })
            }
            // A Commodore 64's, unless it loads where another Commodore's BASIC starts.
            Some(Micro6502::C64) => {
                let bytes = crate::asm::output::commodore_program(assembly)?;
                Runnable::from_file(&format!("{stem}.prg"), bytes).ok_or_else(|| crate::i18n::tr("The program couldn't be made.").to_string())
            }
            // An Atari 2600 cartridge: its ROM, from the lowest address to the highest.
            Some(Micro6502::Atari2600) => {
                let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
                    return Err(crate::i18n::tr("Nothing was assembled.").into());
                };
                let bytes = assembly.image(low, high);
                if crate::atari2600::detect(&bytes).is_none() {
                    return Err(crate::i18n::tf!("{} bytes isn't the size of an Atari 2600 cartridge (2K, 4K, 8K, 12K, 16K or 32K, up to the vectors at $FFFC).", bytes.len()));
                }
                Ok(Runnable { machine: Machine::Atari2600, format: Format::A26, name: format!("{stem}.a26"), bytes: Arc::new(bytes) })
            }
            // An NES cartridge, with its iNES header.
            Some(Micro6502::Nes) => {
                let bytes = crate::nes::cartridge(assembly, assembled.source.as_deref())?;
                Ok(Runnable { machine: Machine::Nes, format: Format::Nes, name: format!("{stem}.nes"), bytes: Arc::new(bytes) })
            }
            None => Err(ASK_6502.to_string()),
        };
    }
    // A ZX81's: said so in the source, or put where a ZX81 program's code goes.
    let said = assembled.source.as_deref().and_then(|text| machine_z80(text.lines()));
    if said == Some(Machine::Zx81) || (said.is_none() && assembly.lowest().is_some_and(crate::zx81_program::is_zx81_code)) {
        let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
            return Err(crate::i18n::tr("Nothing was assembled.").into());
        };
        let image = assembly.image(low, high);
        let bytes = if low == i64::from(crate::zx81::FILE_START) {
            // The source made the whole program, its system variables and all.
            crate::basic_extra::zx81_skip(&image).map_err(|problem| crate::i18n::tf!("This is put at $4009, as a whole ZX81 program, but it isn't one: {problem}"))?;
            image
        } else {
            let start = assembly.start().unwrap_or(low);
            crate::zx81_program::machine_code(&image, u16::try_from(low).unwrap_or(0), u16::try_from(start).unwrap_or(0))?
        };
        return Ok(Runnable { machine: Machine::Zx81, format: Format::P, name: format!("{stem}.p"), bytes: Arc::new(bytes) });
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

/// Whether a document is C (or C++) for the Amiga: a C file that includes the Amiga's own headers near its top.
fn amiga_c_source(doc: &super::Document) -> bool {
    let name = doc.name().to_ascii_lowercase();
    if ![".c", ".h", ".cpp", ".cc", ".cxx", ".hpp"].iter().any(|e| name.ends_with(e)) {
        return false;
    }
    (0..doc.text.line_count().min(150)).any(|n| {
        let line = doc.text.line(n);
        let line = line.trim_start();
        line.starts_with("#include") && ["<exec/", "<proto/", "<clib/", "<intuition/", "<dos/", "<graphics/", "<libraries/", "<workbench/", "<devices/", "<pragmas/", "<inline/"].iter().any(|h| line.contains(h))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assemblers::assemble_own;

    /// What F9 makes of the Game Boy's and the NES's starters, and of SPC700 code: a cartridge each to run, and the
    /// sound processor's bytes to save.
    #[test]
    fn f9_makes_game_boy_and_nes_cartridges() {
        let made = |cpu: Cpu, text: &str| made_from(cpu, &assemble_own(cpu, Path::new("p.asm"), text), "p", Micro::default());
        let Ok(Made::Run(gb)) = made(Cpu::Sm83, crate::gameboy::STARTER) else { panic!("a Game Boy cartridge") };
        assert_eq!((gb.machine, gb.format, gb.name.as_str(), gb.bytes.len()), (Machine::GameBoy, Format::Gb, "p.gb", 0x8000));
        assert!(crate::gameboy::header(&gb.bytes).unwrap().checksum_right);
        let Ok(Made::Run(nes)) = made(Cpu::M6502, crate::nes::STARTER) else { panic!("an NES cartridge") };
        assert_eq!((nes.machine, nes.format, nes.name.as_str()), (Machine::Nes, Format::Nes, "p.nes"));
        let Ok(Made::Saved { name, bytes }) = made(Cpu::Spc700, "  org $0200\nloop: bra loop\n") else { panic!("SPC700 bytes") };
        assert_eq!((name.as_str(), bytes), ("p.bin", vec![0x2F, 0xFE]));
        // An NES program is known by its code too, with nothing said.
        let unsaid = "  org $C000\nreset: lda #0\n  sta $2001\nloop: jmp loop\n  org $FFFA\n  .word reset,reset,reset\n";
        let Ok(Made::Run(nes)) = made(Cpu::M6502, unsaid) else { panic!("an NES cartridge") };
        assert_eq!(nes.machine, Machine::Nes);
    }
}
