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
    /// The file being renamed, by its place in the list, and its new name as typed so far.
    pub renaming: Option<(usize, String)>,
}

impl Limoniter {
    pub(super) fn open_dragon_disk(&mut self) {
        if self.dragon_disk.is_none() {
            self.dragon_disk = Some(Box::new(DragonDisk { disk: Disk::new(40, 1), path: None, said: String::new(), renaming: None }));
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
        let (mut remove, mut save_out, mut renamed) = (None, None, None);
        let files = state.disk.files();
        let free = (0..state.disk.sector_count()).filter(|l| state.disk.is_free(*l)).count();
        crate::fit::Dialog::new(crate::i18n::tr("Dragon DOS disk")).collapsible(false).resizable(true).default_width(520.0).show(ctx, |ui| {
            let name = state.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| crate::i18n::tr("A new disk").to_string());
            ui.label(RichText::new(crate::i18n::tf!("{name}: {} tracks, {} sides, {} KB free", state.disk.tracks, state.disk.sides, free * crate::dragon_dos::SECTOR / 1024)).strong());
            ui.horizontal_wrapped(|ui| {
                crate::fit::menu_button(ui, crate::i18n::tr("New disk"), |ui| {
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
            egui::Grid::new("dragon-files").num_columns(6).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                for (n, file) in files.iter().enumerate() {
                    match &mut state.renaming {
                        // Renamed as it's typed over: Enter keeps the new name, Escape leaves it as it was.
                        Some((which, typed)) if *which == n => {
                            let edit = ui.add_sized([130.0, 20.0], egui::TextEdit::singleline(typed).font(egui::TextStyle::Monospace));
                            edit.request_focus();
                            if ui.input(|i| i.key_pressed(Key::Enter)) {
                                renamed = Some((n, typed.clone()));
                            } else if ui.input(|i| i.key_pressed(Key::Escape)) {
                                renamed = Some((n, String::new()));
                            }
                        }
                        _ => {
                            ui.label(RichText::new(format!("{}.{}", file.name, file.ending)).monospace());
                        }
                    }
                    ui.label(crate::i18n::tf!("{} bytes", file.bytes.len()));
                    match crate::dragon_dos::binary_header(&file.bytes) {
                        Some((load, _, exec)) => ui.label(RichText::new(crate::i18n::tf!("loads at ${load:04X}, starts at ${exec:04X}")).color(theme::text_dim())),
                        None => ui.label(""),
                    };
                    if ui.small_button(crate::i18n::tr("Save out\u{2026}")).clicked() {
                        save_out = Some(n);
                    }
                    if ui.small_button(crate::i18n::tr("Rename")).on_hover_text(crate::i18n::tr("Type its new name, then press Enter")).clicked() {
                        state.renaming = Some((n, format!("{}.{}", file.name, file.ending)));
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
            ui.label(RichText::new(crate::i18n::tr("To run a program from it in XRoar: load the disk, then type RUN\"NAME\" for BASIC or RUN\"NAME.BIN\" for machine code. A saved disk opened in Limoniter runs with F9, its first program typed for; and 6809 source saying run from: disk is put on a disk to run.")).size(12.0).color(theme::text_faint()));
            close = ui.button(crate::i18n::tr("Close")).clicked();
        });
        if let Some((tracks, sides)) = fresh {
            state.disk = Disk::new(tracks, sides);
            state.path = None;
            state.said.clear();
            state.renaming = None;
        }
        if open
            && let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Open a Dragon DOS disk")).add_filter(crate::i18n::tr("Dragon disk"), &["vdk"]).pick_file()
        {
            match std::fs::read(&path).ok().and_then(|bytes| Disk::from_vdk(&bytes)) {
                Some(disk) => {
                    state.disk = disk;
                    state.path = Some(path);
                    state.said.clear();
                    state.renaming = None;
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
        if let Some((n, typed)) = renamed {
            state.renaming = None;
            if let Some(file) = files.get(n)
                && !typed.trim().is_empty()
            {
                let (name, ending) = typed.trim().split_once('.').unwrap_or((typed.trim(), file.ending.as_str()));
                state.said = match state.disk.rename(&file.name, &file.ending, name, ending) {
                    Ok(()) => String::new(),
                    Err(problem) => problem,
                };
            }
        }
        if let Some(n) = remove
            && let Some(file) = files.get(n)
        {
            state.disk.remove(&file.name, &file.ending);
            state.renaming = None;
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

    /// Source saying run from: disk is put on a Dragon DOS disk, which XRoar has RUN typed for.
    #[test]
    fn f9_puts_the_program_on_a_disk_when_the_source_says() {
        use super::super::asm_host::{Made, Micro, made_from};
        use crate::assemblers::{Cpu, assemble_own};
        use crate::running::{Format, Machine, Runnable};
        let source = "; machine: dragon64\n; run from: disk\n\torg $4000\nstart\tlda #65\n\tjsr [$A002]\n\trts\n\tend start\n";
        let Ok(Made::Run(run)) = made_from(Cpu::M6809, &assemble_own(Cpu::M6809, Path::new("my game.asm"), source), "my game", Micro::default()) else { panic!("not made") };
        assert_eq!((run.machine, run.format, run.name.as_str()), (Machine::Dragon64, Format::Vdk, "my game.vdk"));
        let disk = Disk::from_vdk(&run.bytes).expect("a Dragon DOS disk");
        let files = disk.files();
        assert_eq!(files.len(), 1);
        assert_eq!(format!("{}.{}", files[0].name, files[0].ending), "MYGAME.BIN");
        assert_eq!(crate::dragon_dos::binary_header(&files[0].bytes), Some((0x4000, 7, 0x4000)));
        // Said otherwise, it's the binary, as before.
        let Ok(Made::Run(binary)) = made_from(Cpu::M6809, &assemble_own(Cpu::M6809, Path::new("g.asm"), &source.replace("disk", "binary")), "g", Micro::default()) else { panic!("not made") };
        assert_eq!((binary.format, binary.bytes.to_vec()), (Format::CocoBin, files[0].bytes.clone()));
        // A saved disk opened again is known as the Dragon's, and runs in XRoar from drive 1 with RUN typed.
        let opened = Runnable::from_file("my game.vdk", run.bytes.to_vec()).expect("known");
        assert_eq!((opened.machine, opened.format), (Machine::Dragon32, Format::Vdk));
        assert!(Runnable::from_file("other.vdk", vec![0; 4096]).is_none(), "not a Dragon DOS disk");
        let xroar = crate::emulators::by_id("xroar").unwrap();
        assert!(xroar.runs(Machine::Dragon64, Format::Vdk));
        let folder = std::env::temp_dir().join(format!("limoniter-dragon-f9-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join(&run.name);
        std::fs::write(&file, run.bytes.as_slice()).unwrap();
        let named = file.display().to_string();
        assert_eq!(xroar.arguments(Machine::Dragon64, Format::Vdk, &file, None, ""), ["-machine", "dragon64", "-cart", "dragondos", "-load-fd0", named.as_str(), "-type", "RUN\"MYGAME.BIN\"\\r"]);
        assert_eq!(xroar.arguments(Machine::Dragon32, Format::CocoBin, Path::new("/tmp/g.bin"), None, ""), ["-machine", "dragon32", "-run", "/tmp/g.bin"], "a binary as before");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_file_is_renamed_in_the_window() {
        let (mut app, ctx) = app_with("");
        app.open_dragon_disk();
        app.dragon_disk.as_mut().unwrap().disk.add("old", "BIN", &crate::dragon_dos::binary(0x3000, 0x3000, &[0x39])).unwrap();
        // As if Rename was pressed and a new name typed; Enter isn't pressed, so it's kept as typed.
        app.dragon_disk.as_mut().unwrap().renaming = Some((0, "NEWNAME.BIN".into()));
        let _ = ctx.run_ui(Default::default(), |ui| app.dragon_disk_window(ui.ctx()));
        assert_eq!(app.dragon_disk.as_ref().unwrap().renaming.as_ref().map(|r| r.1.as_str()), Some("NEWNAME.BIN"));
        // Enter keeps it.
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        let _ = ctx.run_ui(input, |ui| app.dragon_disk_window(ui.ctx()));
        let state = app.dragon_disk.as_ref().unwrap();
        assert!(state.renaming.is_none(), "{}", state.said);
        assert_eq!(state.disk.files().iter().map(|f| format!("{}.{}", f.name, f.ending)).collect::<Vec<_>>(), ["NEWNAME.BIN"]);
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
