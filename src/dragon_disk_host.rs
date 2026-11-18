//! The Dragon DOS disk window: a disk made or opened, its files listed (a binary's load and start addresses with
//! it), files put on it from the computer or from the 6809 program in front, taken off or saved out, and the
//! disk kept as a VDK file, which XRoar and PC-Dragon load.

use super::*;
use crate::dragon_dos::Disk;

pub(super) struct DragonDisk {
    pub disk: Disk,
    /// Where it was opened from or saved to.
    pub path: Option<PathBuf>,
    pub said: String,
}

impl Limoniter {
    pub(super) fn open_dragon_disk(&mut self) {
        if self.dragon_disk.is_none() {
            self.dragon_disk = Some(Box::new(DragonDisk { disk: Disk::new(40, 1), path: None, said: String::new() }));
        }
    }

    /// The 6809 program in front, assembled as a Dragon DOS binary: its name and bytes.
    fn dragon_program(&mut self) -> Result<(String, Vec<u8>), String> {
        if self.doc_cpu(self.active) != Some(crate::assemblers::Cpu::M6809) {
            return Err(crate::i18n::tr("Put 6809 source in front first.").into());
        }
        let doc = &self.docs[self.active];
        let path = doc.path.clone().unwrap_or_else(|| PathBuf::from(&doc.untitled));
        let text = doc.text.all_lines().iter().map(|l| l.as_ref()).collect::<Vec<&str>>().join("\n");
        let assembled = crate::assemblers::assemble_own(crate::assemblers::Cpu::M6809, &path, &text);
        if let Some(problem) = assembled.assembly.problems.first() {
            return Err(crate::i18n::tf!("It doesn't assemble: line {}, {}", problem.line + 1, problem.message));
        }
        let assembly = &assembled.assembly;
        let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
            return Err(crate::i18n::tr("Nothing was assembled.").into());
        };
        let start = assembly.start().unwrap_or(low);
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "PROGRAM".into());
        Ok((stem, crate::dragon_dos::binary(low as u16, start as u16, &assembly.image(low, high))))
    }

    pub(super) fn dragon_disk_window(&mut self, ctx: &Context) {
        let Some(mut state) = self.dragon_disk.take() else { return };
        let (mut close, mut add_files, mut add_program, mut open, mut save, mut fresh) = (false, false, false, false, false, None);
        let (mut remove, mut save_out) = (None, None);
        let files = state.disk.files();
        let free = (0..state.disk.sector_count()).filter(|l| state.disk.is_free(*l)).count();
        egui::Window::new(crate::i18n::tr("Dragon DOS disk")).collapsible(false).resizable(true).default_width(520.0).show(ctx, |ui| {
            let name = state.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| crate::i18n::tr("A new disk").to_string());
            ui.label(RichText::new(crate::i18n::tf!("{name}: {} tracks, {} sides, {} KB free", state.disk.tracks, state.disk.sides, free * crate::dragon_dos::SECTOR / 1024)).strong());
            ui.horizontal_wrapped(|ui| {
                ui.menu_button(crate::i18n::tr("New disk"), |ui| {
                    for (tracks, sides, words) in [(40, 1, "40 tracks, one side (180K)"), (40, 2, "40 tracks, two sides (360K)"), (80, 1, "80 tracks, one side (360K)"), (80, 2, "80 tracks, two sides (720K)")] {
                        if ui.button(crate::i18n::tr(words)).clicked() {
                            fresh = Some((tracks, sides));
                            ui.close();
                        }
                    }
                });
                open = ui.button(crate::i18n::tr("Open\u{2026}")).clicked();
                add_files = ui.button(crate::i18n::tr("Add files\u{2026}")).clicked();
                add_program = ui.button(crate::i18n::tr("Add the program in front")).on_hover_text(crate::i18n::tr("The 6809 source in front, assembled as a Dragon DOS binary")).clicked();
                save = ui.button(crate::i18n::tr("Save the disk\u{2026}")).clicked();
            });
            ui.separator();
            if files.is_empty() {
                ui.label(RichText::new(crate::i18n::tr("No files on it yet.")).color(theme::text_faint()));
            }
            egui::Grid::new("dragon-files").num_columns(5).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                for (n, file) in files.iter().enumerate() {
                    ui.label(RichText::new(format!("{}.{}", file.name, file.ending)).monospace());
                    ui.label(crate::i18n::tf!("{} bytes", file.bytes.len()));
                    match crate::dragon_dos::binary_header(&file.bytes) {
                        Some((load, _, exec)) => ui.label(RichText::new(crate::i18n::tf!("loads at ${load:04X}, starts at ${exec:04X}")).color(theme::text_dim())),
                        None => ui.label(""),
                    };
                    if ui.small_button(crate::i18n::tr("Save out\u{2026}")).clicked() {
                        save_out = Some(n);
                    }
                    if crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Take it off the disk").on_hover_text(crate::i18n::tr("Take it off the disk")).clicked() {
                        remove = Some(n);
                    }
                    ui.end_row();
                }
            });
            if !state.said.is_empty() {
                ui.label(RichText::new(&state.said).color(theme::text_dim()));
            }
            ui.separator();
            ui.label(RichText::new(crate::i18n::tr("To run a program from it in XRoar: load the disk, then type RUN\"NAME\" for BASIC or LOAD\"NAME.BIN\" and EXEC for machine code.")).size(12.0).color(theme::text_faint()));
            close = ui.button(crate::i18n::tr("Close")).clicked();
        });
        if let Some((tracks, sides)) = fresh {
            state.disk = Disk::new(tracks, sides);
            state.path = None;
            state.said.clear();
        }
        if open
            && let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Open a Dragon DOS disk")).add_filter(crate::i18n::tr("Dragon disk"), &["vdk"]).pick_file()
        {
            match std::fs::read(&path).ok().and_then(|bytes| Disk::from_vdk(&bytes)) {
                Some(disk) => {
                    state.disk = disk;
                    state.path = Some(path);
                    state.said.clear();
                }
                None => state.said = crate::i18n::tr("That isn't a Dragon DOS disk (a VDK file).").into(),
            }
        }
        if add_files && let Some(paths) = rfd::FileDialog::new().set_title(crate::i18n::tr("Files to put on the disk")).pick_files() {
            for path in paths {
                let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let ending = path.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "BIN".into());
                let outcome = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| state.disk.add(&stem, &ending, &bytes));
                if let Err(problem) = outcome {
                    state.said = problem;
                }
            }
        }
        if add_program {
            match self.dragon_program() {
                Ok((stem, bytes)) => match state.disk.add(&stem, "BIN", &bytes) {
                    Ok(()) => state.said = crate::i18n::tf!("{stem} is on the disk, as a binary."),
                    Err(problem) => state.said = problem,
                },
                Err(problem) => state.said = problem,
            }
        }
        if let Some(n) = remove
            && let Some(file) = files.get(n)
        {
            state.disk.remove(&file.name, &file.ending);
        }
        if let Some(n) = save_out
            && let Some(file) = files.get(n)
            && let Some(path) = rfd::FileDialog::new().set_file_name(format!("{}.{}", file.name, file.ending)).save_file()
            && let Err(e) = std::fs::write(&path, &file.bytes)
        {
            state.said = e.to_string();
        }
        if save {
            let suggested = state.path.as_ref().and_then(|p| p.file_name()).map_or_else(|| "disk.vdk".to_string(), |n| n.to_string_lossy().into_owned());
            if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Save the Dragon DOS disk")).add_filter(crate::i18n::tr("Dragon disk"), &["vdk"]).set_file_name(suggested).save_file() {
                match std::fs::write(&path, state.disk.to_vdk()) {
                    Ok(()) => {
                        state.said = crate::i18n::tf!("Saved as {}.", path.display());
                        state.path = Some(path);
                    }
                    Err(e) => state.said = e.to_string(),
                }
            }
        }
        if !close {
            self.dragon_disk = Some(state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    /// What F9 makes of 6809 source: a Vectrex cartridge, a Dragon DOS binary, or (as before) a CoCo binary.
    #[test]
    fn f9_makes_each_machines_own_program() {
        use super::super::asm_host::{Made, Micro, made_from};
        use crate::assemblers::{Cpu, assemble_own};
        use crate::running::Machine;
        let made = |text: &str| match made_from(Cpu::M6809, &assemble_own(Cpu::M6809, Path::new("p.asm"), text), "p", Micro::default()) {
            Ok(Made::Run(run)) => Ok((run.machine, run.name, run.bytes.to_vec())),
            Ok(Made::Saved { name, .. }) => Err(name),
            Err(problem) => Err(problem),
        };
        let (machine, name, bytes) = made(crate::vectrex::STARTER).unwrap();
        assert_eq!((machine, name.as_str()), (Machine::Vectrex, "p.vec"));
        assert!(bytes.starts_with(b"g GCE 1982"));
        let (machine, _, bytes) = made(crate::vectrex::DRAGON_STARTER).unwrap();
        assert_eq!(machine, Machine::Dragon32);
        assert_eq!(crate::dragon_dos::binary_header(&bytes).map(|(load, _, exec)| (load, exec)), Some((0x3000, 0x3000)));
        let (machine, ..) = made("; machine: dragon64\n\torg $4000\n\trts\n").unwrap();
        assert_eq!(machine, Machine::Dragon64);
        let (machine, ..) = made("\torg $3000\n\trts\n").unwrap();
        assert_eq!(machine, Machine::CoCo, "unsaid, as before");
        assert!(made("; machine: vectrex\n\torg 0\n\trts\n").unwrap_err().contains("g GCE"));
    }

    #[test]
    fn the_program_in_front_goes_on_a_disk_as_a_binary() {
        let (mut app, ctx) = app_with("");
        app.new_document();
        app.doc().text.set_content(vec!["; machine: dragon32".into(), "\torg $3F00".into(), "start\tlda #65".into(), "\tjsr [$A002]".into(), "\trts".into(), "\tend start".into()]);
        crate::highlight::load_now();
        let id = crate::highlight::engine().and_then(|e| e.names().iter().find(|(n, _)| n == "Assembly 6809").map(|(_, s)| *s)).unwrap();
        app.doc().set_language(Language::Named(id));
        app.doc().update_syntax();
        let (stem, bytes) = app.dragon_program().unwrap();
        assert_eq!(crate::dragon_dos::binary_header(&bytes), Some((0x3F00, 7, 0x3F00)), "{stem}");
        app.open_dragon_disk();
        app.dragon_disk.as_mut().unwrap().disk.add(&stem, "BIN", &bytes).unwrap();
        let _ = ctx.run_ui(Default::default(), |ui| app.dragon_disk_window(ui.ctx()));
        assert_eq!(app.dragon_disk.as_ref().unwrap().disk.files()[0].bytes, bytes);
    }
}
