//! The window's side of the Atari ST in Hatari: Profile in Hatari (a run whose cycles Hatari's profiler counts,
//! shown beside the source by label and by line when it's done) and Make a bootable ST floppy. See `st_run`.

use super::*;
use crate::running::{Format, Machine};
use crate::st_run::{self, Summary};
use eframe::egui::Ui;

/// What's kept between frames.
#[derive(Default)]
pub(super) struct StRuns {
    /// The document whose next run in Hatari is profiled.
    pub asked: Option<u64>,
    /// Profiled runs going on: where, the document, and where Hatari saves the profile.
    pub profiling: Vec<(String, u64, PathBuf)>,
    /// The last profile, shown beside its document.
    pub last: Option<Box<Shown>>,
}

pub(super) struct Shown {
    doc_id: u64,
    file: PathBuf,
    outcome: Result<Summary, String>,
}

impl Limoniter {
    /// The Hatari set up in Devices: the one Atari ST programs run on by default, or else the first.
    fn hatari_device(&self) -> Option<usize> {
        let hatari = |n: &usize| matches!(&self.sends.devices[*n].place, crate::deploy::Place::Emulator { emulator, .. } if emulator == "hatari");
        let usual = self.running.defaults.iter().find(|(machine, _)| machine == Machine::AtariSt.id()).and_then(|(_, name)| self.sends.devices.iter().position(|d| d.name == *name)).filter(hatari);
        usual.or_else(|| (0..self.sends.devices.len()).find(hatari))
    }

    /// The Atari program the file in front is, or makes when assembled.
    fn st_program(&mut self, title: &str) -> Option<crate::running::Runnable> {
        match self.runnable() {
            Ok(Some(run)) if run.machine == Machine::AtariSt && run.format == Format::Tos => Some(run),
            Ok(Some(run)) => {
                self.message(title, format!("{} isn't an Atari ST program. Assemble 68000 source with no ORG, or open a .PRG or .TOS.", run.name));
                None
            }
            Ok(None) => {
                self.message(title, "This isn't an Atari ST program. Assemble 68000 source with no ORG, or open a .PRG or .TOS.".into());
                None
            }
            Err(problem) if problem.is_empty() => None,
            Err(problem) => {
                self.message(title, problem);
                None
            }
        }
    }

    /// Profile in Hatari: runs the program in Hatari with its profiler on, from the program's start until it ends
    /// or the time set in Devices is up, and then shows where its cycles went.
    pub(super) fn profile_in_hatari(&mut self) {
        let Some(device) = self.hatari_device() else {
            return self.message("Profile in Hatari", "Hatari isn't set up. Add it in File, Devices (it's at https://www.hatari-emu.org).".into());
        };
        if self.st_program("Profile in Hatari").is_none() {
            return;
        }
        let doc_id = self.docs[self.active].id;
        self.running.st.asked = Some(doc_id);
        self.run_on_target(run_host::Target::Device(device));
        // Unless the add-ons have the run waiting, it's started or failed by now.
        if self.running.pending.is_none() {
            self.running.st.asked = None;
        }
    }

    /// Make a bootable ST floppy: the program in front in the AUTO folder of a 720K floppy (.st, or .msa), with
    /// the boot sector code chosen for Hatari, for a Gotek or a real drive.
    pub(super) fn make_st_floppy(&mut self) {
        let Some(run) = self.st_program("Make a bootable ST floppy") else {
            return;
        };
        let settings = st_run::settings();
        let boot = match &settings.boot_sector {
            Some(path) => match std::fs::read(path) {
                Ok(bytes) => Some(bytes),
                Err(e) => return self.message("Make a bootable ST floppy", format!("The boot sector code {} couldn't be read.\n\n{e}", path.display())),
            },
            None => None,
        };
        let stem = run.name.rsplit_once('.').map_or(run.name.as_str(), |(stem, _)| stem).to_string();
        let Some(path) = rfd::FileDialog::new().set_title("Save the bootable floppy").set_file_name(format!("{stem}.st")).add_filter("Atari ST floppy", &["st", "msa"]).save_file() else {
            return;
        };
        let made = st_run::bootable_floppy(&run.bytes, &run.name, boot.as_deref()).and_then(|disk| {
            let msa = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("msa"));
            let bytes = if msa { st_run::pack_msa(&disk)? } else { disk };
            std::fs::write(&path, bytes).map_err(|e| e.to_string())
        });
        match made {
            Ok(()) => {
                let booting = if boot.is_some() { " Its boot sector runs the code chosen in Devices, under Hatari, first." } else { "" };
                self.message("Make a bootable ST floppy", format!("Saved {}: the program is AUTO\\{} on it, so TOS runs it as the floppy boots.{booting}", path.display(), st_run::atari_name(&run.name)));
            }
            Err(problem) => self.message("Make a bootable ST floppy", format!("The floppy couldn't be made.\n\n{problem}")),
        }
    }

    /// A run in Hatari has ended: if it was profiled, its profile is read and summed up.
    pub(super) fn st_session_ended(&mut self, target: &str) {
        let (ended, going): (Vec<_>, Vec<_>) = std::mem::take(&mut self.running.st.profiling).into_iter().partition(|(t, ..)| t == target);
        self.running.st.profiling = going;
        for (_, doc_id, file) in ended {
            let assembly = self.running.assembled.as_ref().filter(|c| c.doc_id == doc_id).map(|c| c.assembly.clone());
            let outcome = match std::fs::read_to_string(&file) {
                Ok(text) => {
                    let profile = st_run::read_profile(&text);
                    if profile.counted.is_empty() { Err("Hatari's profile had nothing in it.".to_string()) } else { Ok(st_run::summarise(&profile, None, assembly.as_deref())) }
                }
                Err(_) => Err("Hatari was closed before the profile was saved: it's saved when the program ends, or when the time set in Devices, under Hatari, is up.".to_string()),
            };
            self.running.st.last = Some(Box::new(Shown { doc_id, file, outcome }));
        }
    }

    /// The last profile, beside the document it was made from: where the cycles went, by label and by line.
    pub(super) fn st_profile_panel(&mut self, ui: &mut Ui) {
        // A profiled run that was stopped from here ended without being followed to its end.
        let gone: Vec<String> = self.running.st.profiling.iter().filter(|(t, ..)| !self.running.sessions.iter().any(|s| s.target == *t)).map(|(t, ..)| t.clone()).collect();
        for target in gone {
            self.st_session_ended(&target);
        }
        let Some(doc_id) = self.docs.get(self.active).map(|doc| doc.id) else {
            return;
        };
        let Some(shown) = self.running.st.last.as_ref().filter(|s| s.doc_id == doc_id) else {
            return;
        };
        let doc = &self.docs[self.active];
        let line_text = |line: usize| if line < doc.text.line_count() { doc.text.line(line).trim().to_string() } else { String::new() };
        let (mut close, mut go_to_line) = (false, None);
        Panel::right("hatari-profile").default_size(380.0).resizable(true).frame(egui::Frame::new().fill(theme::field()).inner_margin(egui::Margin::symmetric(8, 6))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Profiled in Hatari").color(theme::text_dim()).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| close = ui.small_button("Close").clicked());
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let summary = match &shown.outcome {
                    Ok(summary) => summary,
                    Err(problem) => {
                        ui.label(problem);
                        return;
                    }
                };
                let total = summary.program + summary.elsewhere;
                let share = |cycles: u64, of: u64| if of == 0 { 0.0 } else { cycles as f64 * 100.0 / of as f64 };
                let seconds = if summary.cycles_per_second == 0 { 0.0 } else { total as f64 / summary.cycles_per_second as f64 };
                ui.label(format!("{} cycles in the program, {:.1}% of the {:.2} seconds counted; the rest was TOS and interrupts.", summary.program, share(summary.program, total), seconds));
                if !summary.routines.is_empty() {
                    ui.add_space(4.0);
                    egui::Grid::new("hatari-profile-routines").num_columns(4).spacing([14.0, 2.0]).striped(true).show(ui, |ui| {
                        for heading in ["Label", "Cycles", "Share", "Entered"] {
                            ui.label(RichText::new(heading).strong().size(12.0));
                        }
                        ui.end_row();
                        for routine in summary.routines.iter().take(20) {
                            if crate::access::clickable(ui.add(egui::Label::new(RichText::new(&routine.name).monospace().size(12.0)).sense(egui::Sense::click()))).clicked() {
                                go_to_line = routine.line;
                            }
                            ui.label(RichText::new(routine.cycles.to_string()).monospace().size(12.0));
                            ui.label(RichText::new(format!("{:.1}%", share(routine.cycles, summary.program))).monospace().size(12.0));
                            ui.label(RichText::new(if routine.entered > 0 { routine.entered.to_string() } else { String::new() }).monospace().size(12.0));
                            ui.end_row();
                        }
                    });
                }
                if !summary.lines.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Busiest lines (cycles, share of the program's, times run)").size(12.0).color(theme::text_dim()));
                    for hot in summary.lines.iter().take(15) {
                        let text = format!("{:>5} {:>9} {:>5.1}% {:>7}  {}", hot.line + 1, hot.cycles, share(hot.cycles, summary.program), hot.runs, line_text(hot.line));
                        if crate::access::clickable(ui.add(egui::Label::new(RichText::new(text).monospace().size(12.0)).sense(egui::Sense::click()))).clicked() {
                            go_to_line = Some(hot.line);
                        }
                    }
                } else if !summary.busiest.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Busiest instructions (offset in the program, cycles)").size(12.0).color(theme::text_dim()));
                    for (offset, cycles, text) in summary.busiest.iter().take(25) {
                        let instruction = text.split_whitespace().skip_while(|w| w.chars().all(|c| c.is_ascii_hexdigit())).collect::<Vec<_>>().join(" ");
                        ui.label(RichText::new(format!("+{offset:06X} {cycles:>9}  {instruction}")).monospace().size(12.0));
                    }
                }
                ui.add_space(6.0);
                ui.label(RichText::new(format!("Hatari's own profile, with every address (for its hatari_profile tool): {}", shown.file.display())).size(11.5).color(theme::text_faint()));
            });
        });
        if close {
            self.running.st.last = None;
        }
        if let Some(line) = go_to_line {
            self.doc().text.move_caret(line, 0, false);
            self.doc().text.reveal = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;
    use crate::deploy::{Device, Place};
    use std::time::Duration;

    /// With LIMONITER_HATARI naming Hatari and LIMONITER_EMUTOS a folder of EmuTOS images (and SDL_VIDEODRIVER=dummy
    /// for no window): Profile in Hatari runs 68000 source, and once the program ends its cycles are shown by label
    /// and by line beside the source.
    #[test]
    fn source_is_profiled_in_a_real_hatari() {
        let (Some(hatari), Some(emutos)) = (std::env::var_os("LIMONITER_HATARI"), std::env::var_os("LIMONITER_EMUTOS")) else {
            return;
        };
        let mut settings = st_run::settings();
        settings.set_tos(st_run::Model::St, st_run::emutos_in(Path::new(&emutos), st_run::Model::St, "uk"));
        st_run::set_settings(settings);
        crate::highlight::load_now();
        let (mut app, ctx) = app_with("");
        app.sends.devices = vec![Device { name: "Hatari".into(), place: Place::Emulator { emulator: "hatari".into(), program: hatari.into(), options: "--sound off --fast-forward on".into() }, preset: None, eject: false }];
        app.new_document();
        app.doc().untitled = "spin.s".into();
        let source = ["start:\tmove.w\t#2000,d1", "spin:\tmulu\t#3,d2", "\tdbra\td1,spin", "\tclr.w\t-(sp)", "\ttrap\t#1"];
        app.doc().text.set_content(source.iter().map(|l| l.to_string()).collect());
        app.doc().update_syntax();
        app.assembly_tick();
        app.profile_in_hatari();
        assert!(matches!(app.dialog, Dialog::None), "{:?}", app.script_output);
        assert!(app.running.st.asked.is_none() && app.running.st.profiling.len() == 1);
        let began = std::time::Instant::now();
        while app.running.st.last.is_none() && began.elapsed() < Duration::from_secs(60) {
            app.check_sessions();
            std::thread::sleep(Duration::from_millis(100));
        }
        let shown = app.running.st.last.as_ref().expect("the profile came back");
        let summary = shown.outcome.as_ref().unwrap();
        assert_eq!((summary.routines[0].name.as_str(), summary.routines[0].entered), ("spin", 2001), "{summary:?}");
        assert_eq!(summary.lines[0].line, 1, "MULU is the busiest line");
        ctx.run_ui(Default::default(), |ui| app.st_profile_panel(ui)).textures_delta.clear();
        assert!(app.running.st.last.is_some());
    }
}
