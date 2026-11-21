//! The window's side of compiling: Boriel's ZX Basic and C for the older machines, compiled when they're run
//! (F9) with the compiler installed for their machine, in the background. What the compiler says goes in the
//! Output panel, where clicking a problem goes to it, and problems are marked beside their lines; what it made is
//! saved beside the source and run where that machine's programs run.

use super::*;
use crate::boriel_get;
use crate::compilers::{self, Compiled, Job, Toolchain};
use crate::running::{Machine, Runnable};
use super::run_host::Target;

/// What's being compiled, and what came of the last.
#[derive(Default)]
pub(super) struct Compiling {
    /// The last build: the document, its version then, and what it made or why not (empty when the problems are
    /// in the Output panel).
    done: Option<(u64, u64, Result<Runnable, String>)>,
    /// A build going on: the document, its version, the job, and what will come back.
    job: Option<(u64, u64, Job, std::sync::mpsc::Receiver<Result<Compiled, String>>)>,
    /// Where it runs once it's built, if it was asked for somewhere (Run on); otherwise, as F9 does.
    pub(super) target: Option<Target>,
    /// Boriel's ZX Basic being installed (see `boriel_get`): what's said as it goes, and how it ended.
    installing: Option<std::sync::mpsc::Receiver<Installing>>,
    /// How Boriel's ZX Basic can be installed here, worked out when a box offering it opens (on a Mac, that asks
    /// the Pythons installed for their versions, which isn't done every frame).
    zxbc_way: Option<boriel_get::Way>,
}

/// What comes back from installing Boriel's ZX Basic: a line on how it's going, or the end, with its zxbc and
/// what zxbc --version said.
enum Installing {
    Said(String),
    Done(Result<(PathBuf, String), String>),
}

impl Compiling {
    /// Whether a build is going on.
    pub(super) fn busy(&self) -> bool {
        self.job.is_some()
    }
}

/// What kind of source compiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Compiles {
    Boriel,
    C,
}

impl Limoniter {
    /// Whether the document at this index is source a compiler here makes a program of.
    pub(super) fn compiles(&self, index: usize) -> Option<Compiles> {
        let doc = self.docs.get(index)?;
        if doc.hex_mode || doc.browser.is_some() || doc.picture.is_some() || doc.listing_of.is_some() {
            return None;
        }
        let language = doc.language_name();
        if language == highlight::BASIC_BORIEL {
            return Some(Compiles::Boriel);
        }
        let name = doc.name().to_ascii_lowercase();
        (name.ends_with(".c") || language == "C" && doc.path.is_none()).then_some(Compiles::C)
    }

    /// The program compiled from the source in front: from the last build if the text hasn't changed since,
    /// otherwise (with `report`) a build is started, and an empty error says its result will come.
    pub(super) fn compiled_runnable(&mut self, kind: Compiles, report: bool) -> Result<Runnable, String> {
        let index = self.active;
        let (doc_id, version) = (self.docs[index].id, self.docs[index].text.version());
        if let Some((id, v, result)) = &self.running.compiled.done
            && *id == doc_id
            && *v == version
            && (result.is_ok() || !report)
        {
            return result.clone();
        }
        if !report {
            return Err(crate::i18n::tr("Not compiled yet: F9 compiles it.").into());
        }
        if self.running.compiled.job.as_ref().is_some_and(|(id, v, ..)| *id == doc_id && *v == version) {
            return Err(String::new());
        }
        let job = match self.compile_job(index, kind) {
            Ok(job) => job,
            // No zxbc: a box that offers to install it, rather than only saying where it's got.
            Err(problem) if kind == Compiles::Boriel && problem == compilers::not_found(Machine::Spectrum48, true) => {
                self.running.compiled.zxbc_way = Some(boriel_get::way());
                self.dialog = Dialog::ZxbcMissing(problem);
                return Err(String::new());
            }
            Err(problem) => return Err(problem),
        };
        if self.docs[index].is_modified() {
            if !self.save(index) {
                return Err(crate::i18n::tr("The file couldn't be saved, so it wasn't compiled.").into());
            }
            if self.docs[index].saving.is_some() {
                return Err(crate::i18n::tr("The file is being saved. Run it again once that's finished.").into());
            }
        }
        let version = self.docs[index].text.version();
        // What was marked from the last build goes.
        for doc in &mut self.docs {
            if let Some(intel) = doc.text.intel.as_mut() {
                intel.build_problems.clear();
            }
        }
        let name = self.docs[index].name();
        self.script_print(crate::i18n::tf!("Compiling {name} with {} ({}) for the {}\u{2026}", job.compiler.name, job.program.display(), job.machine.name()), true);
        if !job.flags.is_empty() {
            self.script_print(crate::i18n::tf!("From SCOPTIONS: {}", job.flags.join(" ")), false);
        }
        let (send, receive) = std::sync::mpsc::channel();
        let work = std::env::temp_dir().join(format!("limoniter-compile-{}-{doc_id}", std::process::id()));
        let building = job.clone();
        std::thread::spawn(move || {
            let result = compilers::build(&building, &work);
            let _ = std::fs::remove_dir_all(&work);
            let _ = send.send(result);
        });
        self.running.compiled.job = Some((doc_id, version, job, receive));
        self.running.compiled.target = None;
        Err(String::new())
    }

    /// The job that compiles the document at this index: its compiler (the one its `// compiler:` line asks for,
    /// or the first installed for its machine), its files and where it's built. Why not, if it can't be.
    fn compile_job(&self, index: usize, kind: Compiles) -> Result<Job, String> {
        let doc = &self.docs[index];
        let lines = doc.text.all_lines();
        let text: Vec<&str> = lines.iter().map(|l| l.as_ref()).collect();
        let (machine, basic) = match kind {
            Compiles::Boriel => (Machine::Spectrum48, true),
            Compiles::C => match compilers::c_machine(text.iter().copied()) {
                Some(machine) => (machine, false),
                None => return Err(crate::i18n::tr("Which machine is this C for? Put a line such as // machine: c64 near its top (c64, c128, vic20, plus4, pet, spectrum, sms, gamegear, coleco, megadrive, amiga or atari2600), or include the machine's own header, and F9 compiles it with the compiler installed for it.").into()),
            },
        };
        let said = if basic { None } else { compilers::compiler_said(text.iter().copied()) };
        let (compiler, program) = match said {
            Some(compiler) if !compiler.machines.contains(&machine) => return Err(crate::i18n::tf!("{} doesn't compile for the {}.", compiler.name, machine.name())),
            Some(compiler) => match compiler.find(machine) {
                Some(program) => (compiler, program),
                None => return Err(crate::i18n::tf!("{} wasn't found on this computer. Get it from {}, or take out the // compiler: line to use another.", compiler.name, compiler.website)),
            },
            None => {
                let mut candidates = compilers::for_machine(machine, basic);
                // A SAS/C project is built by SAS/C itself, through vamos, when both are on this computer.
                if machine == Machine::Amiga && doc.path.as_deref().and_then(Path::parent).is_some_and(|folder| crate::sasc::read_smakefile(folder).is_some() || folder.join("SCOPTIONS").is_file() || folder.join("scoptions").is_file()) || crate::sasc::looks_like_sasc(&text.join("\n")) {
                    candidates.sort_by_key(|c| c.toolchain != Toolchain::SasC);
                }
                candidates.into_iter().find_map(|c| c.find(machine).map(|p| (c, p))).ok_or_else(|| compilers::not_found(machine, basic))?
            }
        };
        let Some(main) = doc.path.clone() else {
            return Err(crate::i18n::tf!("Save the file first, so {} can read it.", compiler.name));
        };
        let main = std::path::absolute(&main).unwrap_or(main);
        let folder = main.parent().map(Path::to_path_buf).unwrap_or_default();
        let stem = main.file_stem().map_or_else(|| "program".to_string(), |s| s.to_string_lossy().into_owned());
        let mut job = Job { compiler, program, machine, main: main.clone(), sources: vec![main.clone()], folder: folder.clone(), flags: Vec::new(), libs: Vec::new(), stem, sasc: false };
        if basic {
            return Ok(job);
        }
        job.sources = compilers::c_sources(&main);
        // SGDK builds the project the file is in: the folder above src.
        if compiler.toolchain == Toolchain::Sgdk && folder.file_name().is_some_and(|n| n.eq_ignore_ascii_case("src")) {
            job.folder = folder.parent().map(Path::to_path_buf).unwrap_or(folder.clone());
        }
        // SAS/C itself builds its project unchanged: only the program's name is wanted, from the smakefile or
        // SCOPTIONS, to find what it made.
        if compiler.toolchain == Toolchain::SasC {
            if let Some(project) = crate::sasc::read_smakefile(&folder) {
                job.sources = project.sources;
                job.stem = project.name.unwrap_or(job.stem);
            }
            if let Some(name) = std::fs::read_to_string(folder.join("SCOPTIONS")).or_else(|_| std::fs::read_to_string(folder.join("scoptions"))).ok().and_then(|text| crate::sasc::read_scoptions(&text, compiler.toolchain, &folder).name) {
                job.stem = name;
            }
            return Ok(job);
        }
        // An Amiga project Limoniter keeps makefiles for is built as they build it: its files, its program's name,
        // its AmigaOS's configuration or C library, and its libraries.
        let kept = if matches!(compiler.toolchain, Toolchain::AmigaGcc | Toolchain::Vbcc) { crate::amiga_project::f9_build(&main).filter(|b| b.sources.iter().any(|s| same_path(s, &main))) } else { None };
        if let Some(build) = &kept {
            job.sources = build.sources.clone();
            job.stem = build.program.clone();
            (job.flags, job.libs) = if compiler.toolchain == Toolchain::AmigaGcc { build.gcc.clone() } else { build.vbcc.clone() };
            job.folder = build.folder.clone();
        }
        // SAS/C source: its smakefile's files, its SCOPTIONS' flags, through the compatibility header. An smakefile
        // Limoniter writes isn't a sign of SAS/C source, but an imported SAS/C project's own is.
        if matches!(compiler.toolchain, Toolchain::AmigaGcc | Toolchain::Vbcc) {
            let scoptions = std::fs::read_to_string(folder.join("SCOPTIONS")).or_else(|_| std::fs::read_to_string(folder.join("scoptions"))).ok();
            let project = crate::sasc::read_smakefile(&folder).filter(|_| !crate::amiga_project::smakefile_is_kept(&folder));
            if scoptions.is_some() || project.is_some() || crate::sasc::looks_like_sasc(&text.join("\n")) {
                job.sasc = true;
                if let Some(project) = project
                    && kept.is_none()
                {
                    job.sources = project.sources;
                    if let Some(name) = project.name {
                        job.stem = name;
                    }
                }
                if let Some(text) = scoptions {
                    let options = crate::sasc::read_scoptions(&text, compiler.toolchain, &folder);
                    job.flags.extend(options.flags);
                    if let Some(name) = options.name
                        && kept.is_none()
                    {
                        job.stem = name;
                    }
                }
            }
        }
        Ok(job)
    }

    /// When a build finishes: what it said, its problems marked, the program saved beside the source, and run if
    /// that's what it was for.
    pub(super) fn compile_tick(&mut self, ctx: &Context) {
        self.zxbc_install_tick(ctx);
        let Some((_, _, _, receive)) = &self.running.compiled.job else {
            return;
        };
        let result = match receive.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(crate::i18n::tr("The compiler stopped unexpectedly.").into()),
        };
        let Some((doc_id, version, job, _)) = self.running.compiled.job.take() else {
            return;
        };
        let runnable = match result {
            Ok(compiled) => self.report_compiled(doc_id, &job, compiled),
            Err(problem) => {
                self.script_print(problem.clone(), false);
                Err(problem)
            }
        };
        self.running.compiled.done = Some((doc_id, version, runnable.clone()));
        let target = self.running.compiled.target.take();
        match runnable {
            // Run, if the file built is still the one in front.
            Ok(_) if self.docs.get(self.active).is_some_and(|d| d.id == doc_id) => match target {
                Some(target) => self.run_on_target(target),
                None => self.run_on_machine(),
            },
            Err(problem) if !problem.is_empty() => self.message("Run on machine", problem),
            _ => {}
        }
    }

    /// What a build said, in the Output panel and by the lines of the files open; and the program made, saved
    /// beside the source. The program ready to run, or an empty error when the problems have been shown.
    fn report_compiled(&mut self, doc_id: u64, job: &Job, compiled: Compiled) -> Result<Runnable, String> {
        let errors = compiled.problems.iter().filter(|p| !p.warning).count();
        if !compiled.ok() {
            // What the compiler printed, as it printed it, besides the problems read from it.
            for line in compiled.printed.iter().filter(|l| compilers::problem_in(l).is_none() && !l.trim().is_empty()) {
                self.script_print(line.clone(), false);
            }
        }
        let whole = |file: &str| {
            let path = Path::new(file);
            if path.is_absolute() { path.to_path_buf() } else { job.folder.join(path) }
        };
        let mut marked: Vec<(PathBuf, usize, String)> = Vec::new();
        for (problem, kind) in compiled.problems.iter().map(|p| (p, if p.warning { "warning" } else { "" })).chain(compiled.notes.iter().map(|p| (p, "note"))) {
            if problem.line == 0 {
                self.script_print(problem.message.clone(), false);
                continue;
            }
            let path = whole(&problem.file);
            // Errors as assemblers print them; warnings and notes as gcc does, which the Output panel also follows.
            let line = if kind.is_empty() { format!("{}({}): {}", path.display(), problem.line, problem.message) } else { format!("{}:{}: {kind}: {}", path.display(), problem.line, problem.message) };
            self.script_print(line, false);
            let words = if kind.is_empty() { problem.message.clone() } else { format!("{kind}: {}", problem.message) };
            marked.push((path, problem.line, words));
        }
        for doc in &mut self.docs {
            let Some(path) = doc.path.clone() else { continue };
            let mine: Vec<&(PathBuf, usize, String)> = marked.iter().filter(|(p, ..)| same_path(p, &path) || std::path::absolute(&path).is_ok_and(|a| same_path(p, &a))).collect();
            if mine.is_empty() {
                continue;
            }
            let intel = doc.text.intel.get_or_insert_with(Box::default);
            for (_, line, words) in mine {
                if !intel.build_problems.iter().any(|(l, _)| *l + 1 == *line) {
                    intel.build_problems.push((line - 1, words.clone()));
                }
            }
        }
        let Some((name, bytes)) = compiled.made.clone().filter(|_| compiled.ok()) else {
            self.script_print(if errors == 1 { crate::i18n::tr("1 problem.").to_string() } else { crate::i18n::tf!("{errors} problems.") }, false);
            // The first problem in the file in front, to see straight away.
            if let Some(index) = self.docs.iter().position(|d| d.id == doc_id)
                && let Some(first) = compiled.problems.iter().find(|p| !p.warning && p.line > 0 && self.docs[index].path.as_deref().is_some_and(|d| same_path(&whole(&p.file), &std::path::absolute(d).unwrap_or(d.to_path_buf()))))
            {
                self.docs[index].text.move_caret(first.line - 1, 0, false);
            }
            return Err(String::new());
        };
        // What it made goes beside the source, as the compiler would leave it.
        let beside = job.main.parent().map(|f| f.join(&name));
        if let Some(path) = &beside {
            match crate::codec::write_file(path, &bytes) {
                Ok(()) => {
                    self.script_print(crate::i18n::tf!("Made {} ({} bytes) for the {}.", path.display(), bytes.len(), job.machine.name()), false);
                    // An Amiga program gets its Workbench icon beside it, when that's asked for.
                    self.icon_after_build(doc_id, &name, &bytes, true);
                }
                Err(e) => self.script_print(crate::i18n::tf!("{} couldn't be saved: {e}", path.display()), false),
            }
        }
        compilers::runnable(job, &name, bytes).inspect_err(|problem| self.script_print(problem.clone(), false))
    }
}

impl Limoniter {
    /// Opens the box saying which compilers are installed, for which machines, and where to get the rest.
    pub(super) fn open_compilers_box(&mut self) {
        let found: Vec<(&'static compilers::Compiler, Option<PathBuf>)> = compilers::KNOWN.iter().map(|c| (c, c.machines.iter().find_map(|m| c.find(*m)))).collect();
        if found.iter().any(|(c, path)| c.toolchain == Toolchain::Zxbc && path.is_none()) {
            self.running.compiled.zxbc_way = Some(boriel_get::way());
        }
        self.dialog = Dialog::Compilers(found);
    }

    /// Starts installing Boriel's ZX Basic in the background, the way worked out for this computer, saying how
    /// it's going in the Output panel. With no way to install it, its download page is opened.
    pub(super) fn install_zxbc(&mut self) {
        if self.running.compiled.installing.is_some() {
            return;
        }
        let way = self.running.compiled.zxbc_way.clone().unwrap_or_else(boriel_get::way);
        if way == boriel_get::Way::Page {
            if let Err(problem) = crate::platform::open_web_page(boriel_get::ARCHIVE_PAGE) {
                warn(&problem);
            }
            return;
        }
        let Some(home) = boriel_get::folder() else {
            return self.message("Boriel's ZX Basic", crate::i18n::tr("There's nowhere to keep it: Limoniter's settings folder couldn't be found.").into());
        };
        self.script_print(crate::i18n::tr("Installing Boriel's ZX Basic\u{2026}").into(), true);
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let said = send.clone();
            let result = boriel_get::install(&way, &home, &|line| {
                let _ = said.send(Installing::Said(line));
            });
            let _ = send.send(Installing::Done(result));
        });
        self.running.compiled.installing = Some(receive);
    }

    /// Whether Boriel's ZX Basic is being installed.
    pub(super) fn installing_zxbc(&self) -> bool {
        self.running.compiled.installing.is_some()
    }

    /// While Boriel's ZX Basic is being installed: what it says, in the Output panel, and when it's done, that it
    /// is (F9 finds it from then on) or why not.
    fn zxbc_install_tick(&mut self, ctx: &Context) {
        let Some(receive) = &self.running.compiled.installing else {
            return;
        };
        let mut said = Vec::new();
        let mut done = None;
        loop {
            match receive.try_recv() {
                Ok(Installing::Said(line)) => said.push(line),
                Ok(Installing::Done(result)) => {
                    done = Some(result);
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    done = Some(Err(crate::i18n::tr("The installation stopped unexpectedly.").into()));
                    break;
                }
            }
        }
        for line in said {
            self.script_print(line, false);
        }
        let Some(result) = done else {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        };
        self.running.compiled.installing = None;
        let words = match result {
            Ok((program, version)) => crate::i18n::tf!("Boriel's ZX Basic is installed ({version}), as {}. F9 compiles with it now.", program.display()),
            Err(problem) => crate::i18n::tf!("Boriel's ZX Basic couldn't be installed: {problem}"),
        };
        self.script_print(words.clone(), false);
        // The Compilers box shows it found; otherwise it's said, unless another box is open.
        match self.dialog {
            Dialog::Compilers(_) => self.open_compilers_box(),
            Dialog::None | Dialog::ZxbcMissing(_) => self.message("Boriel's ZX Basic", words),
            _ => {}
        }
    }

    /// The box F9 opens when zxbc isn't found: why, how it can be installed here, and a button that does it.
    fn zxbc_missing_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::ZxbcMissing(body) = &self.dialog else {
            return false;
        };
        let installing = self.installing_zxbc();
        let way = self.running.compiled.zxbc_way.clone().unwrap_or(boriel_get::Way::Page);
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut install = false;
        egui::Window::new(crate::i18n::tr("Run on machine")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.label(body.as_str());
            ui.add_space(6.0);
            if installing {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(crate::i18n::tr("Boriel's ZX Basic is being installed. The Output panel shows how it's going."));
                });
            } else {
                ui.add(egui::Label::new(RichText::new(boriel_get::explain(&way)).color(theme::text_dim()).size(12.0)).wrap());
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if !installing {
                    let label = if way == boriel_get::Way::Page { crate::i18n::tr("Open its download page") } else { crate::i18n::tr("Install Boriel ZX Basic") };
                    install = ui.button(label).clicked();
                }
                close |= ui.button(crate::i18n::tr("Close")).clicked();
            });
        });
        if install {
            self.install_zxbc();
            close = true;
        }
        if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }

    pub(super) fn compilers_dialog(&mut self, ctx: &Context) -> bool {
        if matches!(self.dialog, Dialog::ZxbcMissing(_)) {
            return self.zxbc_missing_dialog(ctx);
        }
        let Dialog::Compilers(found) = &self.dialog else {
            return false;
        };
        let installing = self.installing_zxbc();
        let way = self.running.compiled.zxbc_way.clone().unwrap_or(boriel_get::Way::Page);
        let mut install = false;
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        egui::Window::new(crate::i18n::tr("Compilers")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(560.0);
            ui.label(crate::i18n::tr("Boriel's ZX Basic and C for the older machines are compiled when they're run (F9), with the first compiler installed for their machine, and the program made is run there. C says its machine with a line such as // machine: c64 near its top (or by the machine's own header), and may ask for a compiler with // compiler: llvm-mos."));
            ui.add_space(6.0);
            egui::Grid::new("compilers-found").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
                for (compiler, path) in found {
                    ui.strong(compiler.name);
                    let machines: Vec<&str> = compiler.machines.iter().map(|m| m.name()).collect();
                    ui.label(RichText::new(machines.join(", ")).color(theme::text_dim()).size(12.0));
                    match path {
                        Some(path) => {
                            ui.label(path.display().to_string());
                        }
                        // Boriel's ZX Basic, Limoniter can install itself.
                        None if compiler.toolchain == Toolchain::Zxbc && installing => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(crate::i18n::tr("Installing\u{2026}"));
                            });
                        }
                        None if compiler.toolchain == Toolchain::Zxbc && way != boriel_get::Way::Page => {
                            ui.horizontal(|ui| {
                                install = ui.button(crate::i18n::tr("Install Boriel ZX Basic")).on_hover_text(boriel_get::explain(&way)).clicked();
                                ui.hyperlink_to(crate::i18n::tr("Not found: get it"), compiler.website).on_hover_text(compiler.website);
                            });
                        }
                        None => {
                            ui.hyperlink_to(crate::i18n::tr("Not found: get it"), compiler.website).on_hover_text(compiler.website);
                        }
                    }
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new(crate::i18n::tr("Each is looked for on the PATH, in the folder its own variable names (CC65_HOME, Z88DK, GDK, VBCC), where its installer puts it, and in a folder named for it in Program Files, Downloads or on the Desktop.")).color(theme::text_dim()).size(12.0)).wrap());
            ui.add_space(6.0);
            close |= ui.button(crate::i18n::tr("OK")).clicked();
        });
        if install {
            self.install_zxbc();
        }
        if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with(name: &str, text: &str) -> Limoniter {
        highlight::load_now();
        let (mut app, _ctx) = super::super::tests::app_with("");
        app.new_document();
        app.doc().untitled = name.into();
        app.doc().text.set_content(text.lines().map(str::to_string).collect());
        app.doc().update_syntax();
        app
    }

    #[test]
    fn boriel_and_c_sources_are_compiled_and_others_are_not() {
        let app = app_with("game.bas", crate::boriel::STARTER);
        assert_eq!(app.docs[app.active].language_name(), highlight::BASIC_BORIEL, "a .bas in Boriel's dialect is coloured as it");
        assert_eq!(app.compiles(app.active), Some(Compiles::Boriel));
        let app = app_with("listing.bas", "10 BORDER 1\n20 GO TO 10");
        assert_eq!(app.compiles(app.active), None, "a Sinclair listing is tokenised, not compiled");
        let app = app_with("hello.c", crate::c_starters::C64);
        assert_eq!(app.compiles(app.active), Some(Compiles::C));
        let app = app_with("notes.txt", "hello");
        assert_eq!(app.compiles(app.active), None);
    }

    #[test]
    fn the_compilers_box_lists_every_compiler() {
        let mut app = app_with("hello.c", "");
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.run(Command::Compilers, ui));
        let Dialog::Compilers(found) = &app.dialog else { panic!("it opens") };
        assert_eq!(found.len(), compilers::KNOWN.len());
    }

    /// F9 on Boriel's ZX Basic with no zxbc opens a box offering to install it; how the installing goes is said
    /// in the Output panel, and its end in a box (or the Compilers box, if that's open, shows it found).
    #[test]
    fn f9_without_zxbc_offers_to_install_it_and_says_how_it_went() {
        let mut app = app_with("game.bas", crate::boriel::STARTER);
        if compilers::by_id("zxbc").unwrap().find(Machine::Spectrum48).is_none() {
            assert_eq!(app.compiled_runnable(Compiles::Boriel, true).err(), Some(String::new()), "nothing more to say than the box");
            let Dialog::ZxbcMissing(body) = &app.dialog else { panic!("the box offering to install it") };
            assert!(body.contains("github.com/boriel-basic/zxbasic"), "{body}");
            assert!(app.running.compiled.zxbc_way.is_some(), "worked out once, as the box opens");
            let ctx = egui::Context::default();
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                assert!(app.compilers_dialog(ui.ctx()), "drawn");
            });
        }
        // As the background thread tells it.
        let (send, receive) = std::sync::mpsc::channel();
        send.send(Installing::Said("Downloading Boriel's ZX Basic 1.19.0\u{2026}".into())).unwrap();
        app.running.compiled.installing = Some(receive);
        app.dialog = Dialog::None;
        let ctx = egui::Context::default();
        app.zxbc_install_tick(&ctx);
        assert!(app.installing_zxbc(), "still going");
        assert!(app.script_output.iter().any(|l| l.contains("Downloading Boriel's ZX Basic 1.19.0")), "{:?}", app.script_output);
        send.send(Installing::Done(Ok((PathBuf::from("/c/zxbasic/zxbc"), "zxbc.py 1.19.0".into())))).unwrap();
        app.zxbc_install_tick(&ctx);
        assert!(!app.installing_zxbc());
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("zxbc.py 1.19.0") && body.contains("F9")), "said when it's done");
        // It went wrong, with the Compilers box open: that's said in the Output panel, and the box stays.
        let (send, receive) = std::sync::mpsc::channel();
        send.send(Installing::Done(Err("boriel.com couldn't be reached".into()))).unwrap();
        app.running.compiled.installing = Some(receive);
        app.open_compilers_box();
        app.zxbc_install_tick(&ctx);
        assert!(matches!(app.dialog, Dialog::Compilers(_)));
        assert!(app.script_output.iter().any(|l| l.contains("couldn't be installed: boriel.com couldn't be reached")), "{:?}", app.script_output);
        // The thread stopping without saying is an end too.
        let (send, receive) = std::sync::mpsc::channel::<Installing>();
        drop(send);
        app.running.compiled.installing = Some(receive);
        app.zxbc_install_tick(&ctx);
        assert!(!app.installing_zxbc());
    }

    #[test]
    fn c_without_a_machine_or_a_compiler_says_what_to_do() {
        let mut app = app_with("hello.c", "#include <stdio.h>\nint main(void) { return 0; }\n");
        let problem = app.compiled_runnable(Compiles::C, true).unwrap_err();
        assert!(problem.contains("// machine: c64"), "{problem}");
        let mut app = app_with("hello.c", "// machine: amiga\n// compiler: cc65\n");
        let problem = app.compiled_runnable(Compiles::C, true).unwrap_err();
        assert!(problem.contains("cc65 doesn't compile for the Amiga"), "{problem}");
        // Quietly, for a menu, nothing is built.
        let mut app = app_with("hello.c", crate::c_starters::C64);
        assert!(app.compiled_runnable(Compiles::C, false).unwrap_err().contains("Not compiled yet"));
        assert!(app.running.compiled.job.is_none());
    }

    /// With cc65 installed: F9 on the C64 starter compiles it in the background, saves the .prg beside it, and a
    /// broken one has its problem marked by its line.
    #[test]
    fn f9_compiles_c_in_the_background_and_marks_its_problems() {
        if compilers::by_id("cc65").unwrap().find(Machine::C64).is_none() {
            eprintln!("cc65 isn't installed: skipped");
            return;
        }
        let folder = std::env::temp_dir().join(format!("limoniter-f9-c-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("hello.c");
        std::fs::write(&path, crate::c_starters::C64).unwrap();
        let (mut app, _ctx) = super::super::tests::app_with("");
        assert!(app.open_path(&path));
        let ctx = egui::Context::default();
        assert_eq!(app.compiled_runnable(Compiles::C, true).err(), Some(String::new()), "started, its result to come");
        let started = std::time::Instant::now();
        while app.running.compiled.job.is_some() && started.elapsed() < Duration::from_secs(60) {
            app.compile_tick(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        let run = app.compiled_runnable(Compiles::C, false).expect("built");
        assert_eq!((run.machine, run.name.as_str()), (Machine::C64, "hello.prg"));
        assert!(folder.join("hello.prg").is_file(), "saved beside the source");
        assert!(app.script_output.iter().any(|l| l.contains("Compiling hello.c with cc65")), "{:?}", app.script_output);
        // Broken: line 4 is marked.
        let broken = folder.join("broken.c");
        std::fs::write(&broken, "// machine: c64\nint main(void)\n{\n    return y;\n}\n").unwrap();
        assert!(app.open_path(&broken));
        let second = app.compiled_runnable(Compiles::C, true);
        assert_eq!(second.as_ref().err(), Some(&String::new()), "{:?}", second.as_ref().map(|r| r.name.clone()));
        let started = std::time::Instant::now();
        while app.running.compiled.job.is_some() && started.elapsed() < Duration::from_secs(60) {
            app.compile_tick(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        let problems = app.docs[app.active].text.intel.as_ref().map(|i| i.build_problems.clone()).unwrap_or_default();
        assert!(problems.iter().any(|(line, words)| *line == 3 && words.contains("y")), "{problems:?} {:?}", app.script_output);
        assert!(app.script_output.iter().any(|l| crate::assemblers::problem_line(l).is_some_and(|(_, n, _)| n == 4)), "clickable in the Output panel: {:?}", app.script_output);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
