//! The window's side of Set up the Amiga (File, Run on): one box that checks what F9 needs to run Amiga programs in
//! Limoniter's window and does what's still missing in one go. The tools and the PUAE core are downloaded (by the
//! same job F9's box uses), Kickstarts found on this computer or chosen by hand are copied under the names PUAE looks
//! for, or AROS, which PUAE has built in, is chosen when there's no Kickstart, and F9 is told to use all of it.
//! The logic is in `amiga_setup`.

use super::{Limoniter, theme};
use crate::amiga_setup::{self as setup, Found, Installed};
use crate::retro;
use crate::running::Machine;
use eframe::egui::{self, Button, Context, Key, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

/// What's in place already.
#[derive(Default)]
struct Status {
    helper: bool,
    core: bool,
    installed: Vec<Installed>,
    aros: bool,
    /// F9 runs Amiga programs in Limoniter's window, and PUAE has been told which Amiga to be.
    f9: bool,
    /// The model PUAE has been told to be, if it has.
    model: Option<String>,
}

impl Status {
    fn read(running: &super::run_host::Running) -> Status {
        let prefs = retro::Prefs::load();
        let model = prefs.options_for(setup::CORE).into_iter().find(|(k, _)| k == setup::OPTION_MODEL).map(|(_, v)| v);
        let here = running.default_for(Machine::Amiga) == Some(retro::target_name(Machine::Amiga).as_str());
        Status {
            helper: retro::find_helper().is_some(),
            core: retro::installed_core(setup::CORE).is_some(),
            installed: retro::system_folder().map(|s| setup::installed(&s)).unwrap_or_default(),
            aros: setup::aros_chosen(&prefs),
            f9: here && model.is_some(),
            model,
        }
    }

    fn kickstart(&self) -> bool {
        !self.installed.is_empty() || self.aros
    }
}

/// The work going on in the background, and what it says it's at.
struct Job {
    result: Receiver<Result<Vec<String>, String>>,
    said: Arc<Mutex<String>>,
}

pub(super) struct SetupBox {
    status: Status,
    /// Kickstarts found on this computer or chosen by hand, each ticked or not.
    found: Vec<Found>,
    /// Folders to look in for an encrypted ROM's rom.key, besides beside the ROM.
    key_folders: Vec<PathBuf>,
    use_aros: bool,
    job: Option<Job>,
    note: String,
}

/// What the background job is to do.
struct Plan {
    missing: Option<retro::Missing>,
    copies: Vec<PathBuf>,
    key_folders: Vec<PathBuf>,
    system: Option<PathBuf>,
}

/// Downloads what's missing, then copies the Kickstarts, telling `say` what it's at. The downloads are given in so a
/// test can stand in for them. The names of the Kickstarts written or already there, when all has gone well.
fn setup_job(plan: Plan, helper: impl FnOnce() -> Result<PathBuf, String>, core: impl FnOnce(&str) -> Result<PathBuf, String>, say: &dyn Fn(String)) -> Result<Vec<String>, String> {
    if let Some(missing) = plan.missing {
        say(crate::i18n::tr("Downloading what F9 needs\u{2026}").into());
        super::retro_host::install_job(missing, helper, core)?;
    }
    let mut names = Vec::new();
    for path in &plan.copies {
        let shown = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        say(crate::i18n::tf!("Copying {shown}\u{2026}"));
        let system = plan.system.as_deref().ok_or_else(|| crate::i18n::tr("There's nowhere to keep it: Limoniter's settings folder couldn't be found."))?;
        let rom = setup::read_rom(path, &plan.key_folders).map_err(|why| format!("{shown}: {why}"))?;
        setup::write_rom(&rom, system).map_err(|why| format!("{shown}: {why}"))?;
        names.push(rom.known.name.to_string());
    }
    Ok(names)
}

/// The last step, on the window's side: PUAE told which Amiga suits the Kickstart, and F9 told to use it. What to say.
fn finish(running: &mut super::run_host::Running, system: Option<&Path>, aros: bool) -> String {
    let have = system.map(setup::installed).unwrap_or_default();
    let choice = setup::choose(&have, aros || setup::aros_chosen(&retro::Prefs::load()) && have.is_empty());
    if let Some(choice) = &choice {
        let mut prefs = retro::Prefs::load();
        setup::apply(&mut prefs, choice);
        prefs.save();
    }
    running.set_default(Machine::Amiga, &retro::target_name(Machine::Amiga));
    match choice {
        Some(choice) => crate::i18n::tf!("It's ready, using {}. Press F9 in an Amiga program to run it here.", choice.why),
        None => crate::i18n::tr("F9 is set, but PUAE still has no Kickstart: choose Kickstart files, or use AROS.").into(),
    }
}

fn missing_downloads() -> Option<retro::Missing> {
    let missing = retro::Missing { helper: retro::find_helper().is_none(), core: retro::core_for(Machine::Amiga).filter(|c| retro::installed_core(c.core).is_none()) };
    (missing.helper || missing.core.is_some()).then_some(missing)
}

impl Limoniter {
    /// Opens the box, looking at what's in place and what Kickstarts are about.
    pub(super) fn open_amiga_setup(&mut self) {
        let places = crate::amiga_run::usual_places();
        let found = setup::scan(&places, retro::system_folder().as_deref());
        let key_folders = crate::amiga_run::key_folders(&places);
        let status = Status::read(&self.running);
        let use_aros = !status.kickstart() && found.is_empty();
        self.running.amiga_setup = Some(Box::new(SetupBox { status, found, key_folders, use_aros, job: None, note: String::new() }));
    }

    /// Draws the box, and acts on it. It's on top of whatever else is open, so it can be opened from F9's box.
    pub(super) fn amiga_setup_dialog(&mut self, ctx: &Context) {
        let Some(mut state) = self.running.amiga_setup.take() else {
            return;
        };
        // A job that has finished.
        let finished = state.job.as_ref().and_then(|job| job.result.try_recv().ok());
        if state.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if let Some(result) = finished {
            state.job = None;
            state.note = match result {
                Ok(_) => {
                    let aros = state.use_aros && state.status.installed.is_empty();
                    finish(&mut self.running, retro::system_folder().as_deref(), aros)
                }
                Err(problem) => problem,
            };
            self.save_settings_if_changed();
            state.status = Status::read(&self.running);
            state.found.retain(|f| f.what.is_ok() && f.path.is_file() && !state.status.installed.iter().any(|i| f.what.as_ref().is_ok_and(|k| k.file == i.file)));
        }
        let busy = state.job.is_some();
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut choose = false;
        let mut start = false;
        crate::fit::Dialog::new(crate::i18n::tr("Set up the Amiga")).collapsible(false).resizable(true).default_width(560.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show_with_buttons(ctx, |ui, part| match part {
            crate::fit::Part::Body => {
                ui.set_max_width(600.0);
                ui.add(egui::Label::new(crate::i18n::tr("F9 can run Amiga programs in Limoniter's window. This checks what that needs and does whatever is still missing, in one go.")).wrap());
                ui.add_space(6.0);
                let st: &mut SetupBox = &mut state;
                let (status, found, use_aros) = (&st.status, &mut st.found, &mut st.use_aros);
                let mark = |ok: bool| if ok { RichText::new("\u{2714}").color(theme::text_dim()) } else { RichText::new("\u{25CB}").color(theme::text_faint()) };
                ui.horizontal(|ui| {
                    ui.label(mark(status.helper));
                    ui.label(crate::i18n::tr("limoniter-retro, the helper that runs it"));
                    if !status.helper {
                        ui.label(RichText::new(crate::i18n::tr("will be downloaded")).color(theme::text_faint()));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(mark(status.core));
                    ui.label(crate::i18n::tr("The PUAE core, which is the Amiga"));
                    if !status.core {
                        ui.label(RichText::new(crate::i18n::tr("will be downloaded")).color(theme::text_faint()));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(mark(status.kickstart()));
                    ui.label(crate::i18n::tr("A Kickstart, the Amiga's built-in software"));
                });
                ui.indent("kickstarts", |ui| {
                    for have in &status.installed {
                        ui.label(RichText::new(format!("\u{2714} {}", have.name)).color(theme::text_dim())).on_hover_text(have.file);
                    }
                    if status.aros {
                        ui.label(RichText::new(crate::i18n::tr("\u{2714} AROS (free, built in)")).color(theme::text_dim()));
                    }
                    for one in found.iter_mut() {
                        let Ok(known) = &one.what else { continue };
                        let mut words = crate::i18n::tf!("{} (from {})", known.name, one.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()));
                        if one.decrypted {
                            words.push_str(crate::i18n::tr(", decrypted"));
                        }
                        if one.doubled {
                            words.push_str(crate::i18n::tr(", stored twice, written once"));
                        }
                        ui.add_enabled(!busy, egui::Checkbox::new(&mut one.ticked, words)).on_hover_text(crate::i18n::tf!("{}. Copied as {}: the original isn't moved or changed.", one.path.display(), known.file));
                    }
                    for one in found.iter() {
                        if let Err(why) = &one.what {
                            ui.add(egui::Label::new(RichText::new(format!("{}: {why}", one.path.display())).color(theme::problem_mark())).wrap());
                        }
                    }
                    if ui.add_enabled(!busy, Button::new(crate::i18n::tr("Choose Kickstart files\u{2026}"))).on_hover_text(crate::i18n::tr("One or several at once, such as Amiga Forever's ROMs. Their names don't matter: what's in them is looked at")).clicked() {
                        choose = true;
                    }
                    if !status.kickstart() && !found.iter().any(|f| f.ticked && f.what.is_ok()) {
                        ui.add_enabled(!busy, egui::Checkbox::new(use_aros, crate::i18n::tr("Use AROS (free, no Kickstart needed)")));
                        ui.add(egui::Label::new(RichText::new(crate::i18n::tr("AROS is a free copy of the Amiga's system, built into PUAE. It runs most programs that keep to the rules, but not every game or demo: those want a real Kickstart.")).color(theme::text_faint()).size(12.0)).wrap());
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(mark(status.f9));
                    ui.label(crate::i18n::tr("F9 runs Amiga programs in Limoniter's window"));
                    if let Some(model) = &status.model {
                        ui.label(RichText::new(crate::i18n::tf!("as an {model}")).color(theme::text_faint()));
                    }
                });
                ui.add_space(6.0);
                if let Some(job) = &st.job {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        let said = job.said.lock().map(|s| s.clone()).unwrap_or_default();
                        ui.label(said);
                    });
                }
                if !st.note.is_empty() {
                    ui.add(egui::Label::new(RichText::new(st.note.as_str()).color(theme::text_dim())).wrap());
                }
            }
            crate::fit::Part::Buttons => {
                ui.separator();
                let ticked = state.found.iter().any(|f| f.ticked && f.what.is_ok());
                let aros_wanted = state.use_aros && !state.status.kickstart() && !ticked;
                let status = &state.status;
                let something = !status.helper || !status.core || ticked || aros_wanted || !status.f9;
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy && something, Button::new(crate::i18n::tr("Set up"))).on_hover_text(crate::i18n::tr("Does every step that isn't ticked yet")).clicked() {
                        start = true;
                    }
                    close |= ui.button(crate::i18n::tr("Close")).clicked();
                });
            }
        });
        if choose
            && let Some(paths) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Choose Kickstart files")).pick_files()
        {
            for path in paths {
                if state.found.iter().any(|f| f.path == path) {
                    continue;
                }
                state.found.push(Found::read(&path, &state.key_folders));
            }
        }
        if start {
            let copies: Vec<PathBuf> = state.found.iter().filter(|f| f.ticked && f.what.is_ok()).map(|f| f.path.clone()).collect();
            let plan = Plan { missing: missing_downloads(), copies, key_folders: state.key_folders.clone(), system: retro::system_folder() };
            let said = Arc::new(Mutex::new(String::new()));
            let (send, receive) = channel();
            let (heard, ctx2) = (said.clone(), ctx.clone());
            std::thread::spawn(move || {
                let result = setup_job(plan, retro::download_helper, retro::download_core, &|words| {
                    if let Ok(mut said) = heard.lock() {
                        *said = words;
                    }
                    ctx2.request_repaint();
                });
                let _ = send.send(result);
                ctx2.request_repaint();
            });
            state.job = Some(Job { result: receive, said });
            state.note.clear();
        }
        if !close || busy {
            self.running.amiga_setup = Some(state);
        }
    }

    /// For a screenshot: the box opened, with a Kickstart made up of nothing real chosen for it to show.
    pub(super) fn amiga_setup_screenshot_step(&mut self, value: &str) {
        self.open_amiga_setup();
        if let (Some(state), true) = (self.running.amiga_setup.as_mut(), value == "done") {
            state.note = crate::i18n::tr("It's ready, using Kickstart 1.3 (A500, A1000, A2000) (PUAE's A500 model). Press F9 in an Amiga program to run it here.").into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amiga_run::{SIZE_256, SIZE_512};
    use std::cell::RefCell;

    fn rom(size: usize, version: u16, revision: u16) -> Vec<u8> {
        let mut rom = vec![0x5Au8; size];
        rom[0] = 0x11;
        rom[1] = if size == SIZE_256 { 0x11 } else { 0x14 };
        rom[2] = 0x4E;
        rom[3] = 0xF9;
        rom[12..14].copy_from_slice(&version.to_be_bytes());
        rom[14..16].copy_from_slice(&revision.to_be_bytes());
        rom
    }

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("limoniter-amiga-host-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn here(helper: bool, core: bool) -> Option<retro::Missing> {
        let missing = retro::Missing { helper: !helper, core: (!core).then(|| retro::core_for(Machine::Amiga).unwrap()) };
        (missing.helper || missing.core.is_some()).then_some(missing)
    }

    #[test]
    fn downloads_are_made_only_for_what_is_missing_and_kickstarts_are_copied() {
        let folder = scratch("job");
        let system = folder.join("system");
        std::fs::write(folder.join("1.3.rom"), rom(SIZE_256, 34, 5)).unwrap();
        std::fs::write(folder.join("3.1.rom"), rom(SIZE_512, 40, 68)).unwrap();
        let said = RefCell::new(Vec::new());
        let asked = RefCell::new(Vec::new());
        let plan = Plan { missing: here(false, false), copies: vec![folder.join("1.3.rom"), folder.join("3.1.rom")], key_folders: Vec::new(), system: Some(system.clone()) };
        let names = setup_job(
            plan,
            || {
                asked.borrow_mut().push("helper".to_string());
                Ok(PathBuf::from("helper"))
            },
            |core| {
                asked.borrow_mut().push(core.to_string());
                Ok(PathBuf::from("core"))
            },
            &|words| said.borrow_mut().push(words),
        )
        .unwrap();
        assert_eq!(*asked.borrow(), ["helper", "puae"]);
        assert_eq!(names, ["Kickstart 1.3 (A500, A1000, A2000)", "Kickstart 3.1 (A1200)"]);
        assert!(system.join("kick34005.A500").is_file() && system.join("kick40068.A1200").is_file());
        assert!(said.borrow().len() >= 3);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn steps_already_done_are_skipped() {
        let folder = scratch("skip");
        let plan = Plan { missing: here(true, true), copies: Vec::new(), key_folders: Vec::new(), system: Some(folder.join("system")) };
        assert!(plan.missing.is_none());
        let names = setup_job(plan, || panic!("the helper is here"), |_| panic!("the core is here"), &|_| {}).unwrap();
        assert!(names.is_empty());
        // Only the core missing means only the core is fetched.
        let asked = RefCell::new(Vec::new());
        let plan = Plan { missing: here(true, false), copies: Vec::new(), key_folders: Vec::new(), system: None };
        setup_job(plan, || panic!("the helper is here"), |core| {
            asked.borrow_mut().push(core.to_string());
            Ok(PathBuf::new())
        }, &|_| {})
        .unwrap();
        assert_eq!(*asked.borrow(), ["puae"]);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_failed_download_or_a_bad_rom_stops_with_a_plain_reason() {
        let folder = scratch("fail");
        let plan = Plan { missing: here(false, true), copies: Vec::new(), key_folders: Vec::new(), system: None };
        let why = setup_job(plan, || Err("No network.".to_string()), |_| Ok(PathBuf::new()), &|_| {}).unwrap_err();
        assert_eq!(why, "No network.");
        std::fs::write(folder.join("notes.txt"), vec![0u8; SIZE_256]).unwrap();
        let plan = Plan { missing: None, copies: vec![folder.join("notes.txt")], key_folders: Vec::new(), system: Some(folder.join("system")) };
        let why = setup_job(plan, || Ok(PathBuf::new()), |_| Ok(PathBuf::new()), &|_| {}).unwrap_err();
        assert!(why.starts_with("notes.txt: It isn't a Kickstart"), "{why}");
        assert!(!folder.join("system").exists());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn f9_is_set_to_run_the_amiga_in_the_window_with_the_model_that_suits() {
        let folder = scratch("f9");
        let system = folder.join("system");
        std::fs::create_dir_all(&system).unwrap();
        std::fs::write(system.join("kick40068.A1200"), rom(SIZE_512, 40, 68)).unwrap();
        let mut running = super::super::run_host::Running::default();
        let said = finish(&mut running, Some(&system), false);
        assert!(said.contains("Press F9"), "{said}");
        assert_eq!(running.default_for(Machine::Amiga), Some(retro::target_name(Machine::Amiga).as_str()));
        let options = retro::Prefs::load().options_for("puae");
        assert!(options.contains(&("puae_model".to_string(), "A1200".to_string())));
        assert!(options.contains(&("puae_kickstart".to_string(), "auto".to_string())));
        // With no Kickstart and AROS asked for, PUAE is told to use AROS.
        let empty = folder.join("none");
        std::fs::create_dir_all(&empty).unwrap();
        finish(&mut running, Some(&empty), true);
        assert!(setup::aros_chosen(&retro::Prefs::load()));
        // With neither, F9 is still set but the words say what's lacking.
        retro::Prefs::default().save();
        assert!(finish(&mut running, Some(&empty), false).contains("no Kickstart"));
        let _ = std::fs::remove_dir_all(folder);
    }
}
