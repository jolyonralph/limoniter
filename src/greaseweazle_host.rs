//! The Greaseweazle window: whether `gw` is installed (and a button to install or update it), the connection
//! tested, a disk read into an image (with a progress bar, track by track) or an image written to a disk, and the
//! drive's own jobs (its speed, cleaning the heads, erasing a disk). Everything is done by running `gw`.

use super::*;
use crate::greaseweazle::{self as gw, Action, Info, Job, Kind, Progress};
use std::collections::VecDeque;
use std::sync::mpsc::Receiver;

const TITLE: &str = "Greaseweazle";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Tab {
    Setup,
    Read,
    Write,
    Tools,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Setup, Tab::Read, Tab::Write, Tab::Tools];

    fn label(self) -> &'static str {
        match self {
            Tab::Setup => "Set up",
            Tab::Read => "Read a disk",
            Tab::Write => "Write a disk",
            Tab::Tools => "Drive",
        }
    }
}

pub(super) struct GwWindow {
    tab: Tab,
    program: Option<PathBuf>,
    /// What `gw info` last said.
    info: Option<Info>,
    job: Option<Job>,
    /// What runs after the job, in turn.
    queue: VecDeque<Action>,
    /// Something being installed, and the version known to be out.
    install: Option<Receiver<Result<(PathBuf, String), String>>>,
    latest: Option<Receiver<Result<String, String>>>,
    latest_version: Option<String>,
    note: String,
    output: String,
    summary: Option<String>,
    /// Shows every format `gw` knows, with a filter.
    more: bool,
    filter: String,
    save_to: String,
    write_from: String,
    /// What's been asked that wants a yes first.
    confirm: Option<Action>,
}

impl GwWindow {
    fn new(prefs: &gw::Prefs, tab: Tab) -> GwWindow {
        let program = gw::found_cached(prefs);
        let mut window = GwWindow { tab, program, info: None, job: None, queue: VecDeque::new(), install: None, latest: None, latest_version: None, note: String::new(), output: String::new(), summary: None, more: false, filter: String::new(), save_to: String::new(), write_from: String::new(), confirm: None };
        // What's plugged in, and the tools' version, whenever there are tools to ask.
        if window.program.is_some() {
            window.queue.push_back(Action::Info);
        }
        window
    }

    /// Starts the next action waiting, if nothing's running.
    fn next(&mut self, prefs: &gw::Prefs) {
        if self.job.is_some() {
            return;
        }
        let Some(program) = self.program.clone() else {
            self.queue.clear();
            return;
        };
        if let Some(action) = self.queue.pop_front() {
            match gw::check(prefs, &action) {
                Ok(()) => {
                    self.output.clear();
                    self.summary = None;
                    self.job = Some(Job::start(&program, prefs, action));
                }
                Err(problem) => {
                    self.queue.clear();
                    self.summary = Some(problem);
                }
            }
        }
    }

    /// Takes in a finished job's answer, and gives back what was done, whether it worked, and what `gw` said.
    fn finished(&mut self) -> Option<(Action, bool, String)> {
        let job = self.job.as_ref()?;
        let done = job.done()?;
        let text = job.text();
        let action = job.action.clone();
        self.job = None;
        let ok = match done {
            Ok(exited) => !gw::failed(exited, &text),
            Err(problem) => {
                self.summary = Some(problem);
                self.queue.clear();
                return None;
            }
        };
        if action == Action::Info {
            self.info = Some(gw::parse_info(&text));
        }
        if !ok {
            self.queue.clear();
        }
        self.summary = Some(gw::summary(&action, ok, &text));
        // The words stay as they were said: Info's answer is a few lines, and a read's are what its bar followed.
        self.output = text.clone();
        // A good answer to Info is already shown, as the model and firmware, beside the button that asked, unless
        // Drive info itself asked.
        if action == Action::Info && ok && self.tab != Tab::Tools {
            self.output.clear();
            self.summary = None;
        }
        if action == Action::Update && ok {
            self.queue.push_back(Action::Info);
        }
        Some((action, ok, text))
    }

    fn running(&self) -> bool {
        self.job.is_some() || self.install.is_some()
    }

    /// Runs what's waiting to its end, there and then: for a screenshot, which can't wait for frames.
    pub(super) fn settle_now(&mut self, prefs: &gw::Prefs) {
        let began = std::time::Instant::now();
        while began.elapsed() < Duration::from_secs(20) {
            self.next(prefs);
            if self.job.is_none() {
                break;
            }
            while self.job.as_ref().is_some_and(|j| j.done().is_none()) && began.elapsed() < Duration::from_secs(20) {
                std::thread::sleep(Duration::from_millis(10));
            }
            self.finished();
        }
    }
}

/// What the window's buttons asked for.
enum Asked {
    Do(Action),
    /// Wants a yes first.
    Confirm(Action),
    Stop,
    Install,
    CheckLatest,
    /// Look for `gw` again, and ask it about the Greaseweazle.
    Test,
    ChooseProgram,
    ForgetProgram,
    ChooseSaveTo,
    ChooseWriteFrom,
    UseFront,
}

impl Limoniter {
    pub(super) fn open_greaseweazle(&mut self, tab: Tab, action: Option<Action>) {
        match &mut self.gw {
            Some(window) => window.tab = tab,
            None => self.gw = Some(Box::new(GwWindow::new(&self.gw_prefs, tab))),
        }
        let front = self.front_image();
        if let Some(window) = &mut self.gw {
            window.confirm = None;
            if tab == Tab::Write && window.write_from.is_empty() {
                window.write_from = front.map(|p| p.display().to_string()).unwrap_or_default();
            }
            window.program = gw::found_cached(&self.gw_prefs);
            if let Some(action) = action {
                window.queue.retain(|a| *a != action);
                window.queue.push_back(action);
            }
        }
    }

    /// Opens with a question first, for the jobs that wear the drive or change a disk.
    pub(super) fn open_greaseweazle_asking(&mut self, action: Action) {
        self.open_greaseweazle(Tab::Tools, None);
        if let Some(window) = &mut self.gw {
            window.confirm = Some(action);
        }
    }

    /// The file in front, when it's a saved file that could be a disk image.
    fn front_image(&self) -> Option<PathBuf> {
        if self.no_file() {
            return None;
        }
        self.docs[self.active].path.clone().filter(|p| p.is_file())
    }

    pub(super) fn greaseweazle_window(&mut self, ctx: &Context) {
        let Some(mut window) = self.gw.take() else { return };
        let before = self.gw_prefs.clone();
        // An install that's finished.
        if let Some(receiver) = &window.install
            && let Ok(result) = receiver.try_recv()
        {
            window.install = None;
            match result {
                Ok((program, version)) => {
                    window.note = crate::i18n::tf!("Greaseweazle {version} is installed: {}", program.display());
                    window.program = Some(program);
                    window.queue.push_back(Action::Info);
                }
                Err(problem) => window.note = problem,
            }
        }
        if let Some(receiver) = &window.latest
            && let Ok(result) = receiver.try_recv()
        {
            window.latest = None;
            match result {
                Ok(version) => window.latest_version = Some(version),
                Err(problem) => window.note = problem,
            }
        }
        if let Some((action, ok, _)) = window.finished()
            && let Action::Read(path) = action
            && ok
        {
            // The image made opens as any disk image does, in the disk browser.
            if path.is_file() {
                self.open_path(&path);
            }
            window.summary = window.summary.take().map(|s| crate::i18n::tf!("{s} Saved as {}.", path.display()));
        }
        window.next(&self.gw_prefs);
        let front = self.front_image();
        let mut asked = None;
        let mut open = true;
        let prefs = &mut self.gw_prefs;
        crate::fit::Dialog::new(crate::i18n::tr(TITLE)).open(&mut open).resizable(true).default_width(640.0).show(ctx, |ui| {
            asked = draw(ui, &mut window, prefs, front.as_deref());
        });
        if let Some(asked) = asked {
            self.gw_asked(&mut window, asked, front);
        }
        window.next(&self.gw_prefs);
        if window.running() || !window.queue.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if self.gw_prefs != before {
            self.save_settings_if_changed();
        }
        if open {
            self.gw = Some(window);
        } else if let Some(job) = &window.job {
            job.stop();
        }
    }

    fn gw_asked(&mut self, window: &mut GwWindow, asked: Asked, front: Option<PathBuf>) {
        match asked {
            Asked::Do(action) => {
                window.confirm = None;
                let action = match action {
                    Action::Read(_) => Action::Read(self.read_target(window)),
                    Action::Write(_) => Action::Write(PathBuf::from(window.write_from.trim())),
                    other => other,
                };
                window.queue.push_back(action);
            }
            Asked::Confirm(action) => {
                let action = match action {
                    Action::Write(_) => Action::Write(PathBuf::from(window.write_from.trim())),
                    other => other,
                };
                window.confirm = Some(action);
            }
            Asked::Stop => {
                if let Some(job) = &window.job {
                    job.stop();
                }
                window.queue.clear();
            }
            Asked::Install => {
                let (send, receive) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = send.send(gw::install());
                });
                window.install = Some(receive);
                window.note = crate::i18n::tr("Installing\u{2026} this can take a minute.").into();
            }
            Asked::CheckLatest => {
                let (send, receive) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = send.send(gw::latest_version());
                });
                window.latest = Some(receive);
            }
            Asked::Test => {
                window.program = gw::find(&self.gw_prefs);
                window.info = None;
                window.queue.push_back(Action::Info);
            }
            Asked::ChooseProgram => {
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Choose the gw program")).pick_file() {
                    self.gw_prefs.program = Some(path);
                    window.program = gw::find(&self.gw_prefs);
                    window.queue.push_back(Action::Info);
                }
            }
            Asked::ForgetProgram => {
                self.gw_prefs.program = None;
                window.program = gw::find(&self.gw_prefs);
                window.info = None;
            }
            Asked::ChooseSaveTo => {
                let ending = self.gw_prefs.kind.ending(&self.gw_prefs.format);
                let ending = if self.gw_prefs.format.is_empty() { "scp" } else { ending };
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save the disk image")).set_file_name(format!("disk.{ending}")).save_file() {
                    window.save_to = path.display().to_string();
                }
            }
            Asked::ChooseWriteFrom => {
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Choose the disk image to write")).pick_file() {
                    window.write_from = path.display().to_string();
                }
            }
            Asked::UseFront => window.write_from = front.map(|p| p.display().to_string()).unwrap_or_default(),
        }
    }

    /// Where a read is saved: the place chosen, with the ending the kind of image needs.
    fn read_target(&self, window: &GwWindow) -> PathBuf {
        let ending = if self.gw_prefs.format.is_empty() { "scp" } else { self.gw_prefs.kind.ending(&self.gw_prefs.format) };
        PathBuf::from(window.save_to.trim()).with_extension(ending)
    }
}

/// The kind of disk: the short list by name, and every format `gw` has under More.
fn disk_choice(ui: &mut Ui, window: &mut GwWindow, prefs: &mut gw::Prefs, id: &str) {
    let name = gw::COMMON.iter().find(|d| d.format == prefs.format).map(|d| crate::i18n::tr(d.name).to_string()).unwrap_or_else(|| prefs.format.clone());
    egui::ComboBox::from_id_salt(id).selected_text(name).width(240.0).show_ui(ui, |ui| {
        for disk in gw::COMMON {
            if ui.selectable_label(prefs.format == disk.format, crate::i18n::tr(disk.name)).clicked() {
                prefs.format = disk.format.to_string();
                prefs.kind = if disk.format.is_empty() { Kind::Scp } else { Kind::Sectors };
                window.more = false;
            }
        }
        ui.separator();
        if ui.selectable_label(window.more, crate::i18n::tr("More formats\u{2026}")).clicked() {
            window.more = true;
        }
    });
    if window.more {
        ui.add(egui::TextEdit::singleline(&mut window.filter).hint_text(crate::i18n::tr("Type part of its name: amiga, ibm, 1541")).desired_width(240.0));
        let wanted = window.filter.trim().to_ascii_lowercase();
        egui::ScrollArea::vertical().id_salt(format!("{id}-more")).max_height(120.0).show(ui, |ui| {
            ui.set_min_width(240.0);
            for format in gw::all_formats().into_iter().filter(|f| wanted.is_empty() || f.contains(&wanted)) {
                if ui.selectable_label(prefs.format == format, format).clicked() {
                    // From raw flux to a format: the sectors are the usual thing to save.
                    if prefs.format.is_empty() {
                        prefs.kind = Kind::Sectors;
                    }
                    prefs.format = format.to_string();
                }
            }
        });
    }
}

fn draw(ui: &mut Ui, window: &mut GwWindow, prefs: &mut gw::Prefs, front: Option<&Path>) -> Option<Asked> {
    let mut asked = None;
    ui.horizontal(|ui| {
        for tab in Tab::ALL {
            if ui.selectable_label(window.tab == tab, crate::i18n::tr(tab.label())).clicked() {
                window.tab = tab;
                window.confirm = None;
            }
        }
    });
    ui.separator();
    // No gw: what's needed is the setting up, whatever the tab.
    if window.program.is_none() && window.tab != Tab::Setup {
        ui.label(crate::i18n::tr("Greaseweazle's gw program isn't installed. Set up Greaseweazle installs it."));
        if ui.button(crate::i18n::tr("Set up Greaseweazle\u{2026}")).clicked() {
            window.tab = Tab::Setup;
        }
        return asked;
    }
    let running = window.running();
    match window.tab {
        Tab::Setup => asked = draw_setup(ui, window, prefs),
        Tab::Read => asked = draw_read(ui, window, prefs, running),
        Tab::Write => asked = draw_write(ui, window, prefs, front, running),
        Tab::Tools => asked = draw_tools(ui, window, running),
    }
    if let Some(confirm) = window.confirm.clone() {
        ui.add_space(6.0);
        ui.group(|ui| {
            let drive = prefs.drive.clone();
            let (warning, go) = match &confirm {
                Action::Write(_) => (crate::i18n::tf!("Everything on the disk in drive {drive} will be replaced by this image. It can't be undone."), crate::i18n::tr("Write the disk")),
                Action::Erase => (crate::i18n::tf!("Everything on the disk in drive {drive} will be erased. It can't be undone."), crate::i18n::tr("Erase the disk")),
                _ => (crate::i18n::tf!("Put a head-cleaning disk in drive {drive} first: the heads are moved across it a few times, and a disk with data on it would be scratched."), crate::i18n::tr("Clean the heads")),
            };
            ui.colored_label(theme::rainbow_mistake(), warning);
            ui.horizontal(|ui| {
                if ui.add_enabled(!running, egui::Button::new(go)).clicked() {
                    asked = Some(Asked::Do(confirm.clone()));
                }
                if ui.button(crate::i18n::tr("Cancel")).clicked() {
                    window.confirm = None;
                }
            });
        });
    }
    // The progress, what was said, and how it ended.
    let text = window.job.as_ref().map_or_else(|| window.output.clone(), Job::text);
    let progress: Progress = gw::progress(&text);
    if let Some(job) = &window.job {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spinner();
            if matches!(job.action, Action::Read(_) | Action::Write(_) | Action::Erase) {
                let shown = match progress.fraction() {
                    Some(f) => egui::ProgressBar::new(f).desired_width(300.0).text(crate::i18n::tf!("{} of {} tracks ({})", progress.done, progress.total, progress.current)),
                    None => egui::ProgressBar::new(0.0).desired_width(300.0).animate(true),
                };
                ui.add(shown);
            }
            if ui.button(crate::i18n::tr("Stop")).clicked() {
                asked = Some(Asked::Stop);
            }
        });
    }
    if progress.retries > 0 || !progress.bad.is_empty() {
        let (retries, bad) = (progress.retries, progress.bad.len());
        ui.label(RichText::new(crate::i18n::tf!("{retries} read(s) retried; {bad} track(s) with sectors missing")).color(if bad > 0 { theme::rainbow_mistake() } else { theme::text_dim() }));
        for line in &progress.bad {
            ui.label(RichText::new(line).monospace().color(theme::rainbow_mistake()));
        }
    }
    if let Some(summary) = &window.summary {
        ui.add_space(4.0);
        ui.label(RichText::new(summary).strong());
    }
    if !text.is_empty() {
        egui::ScrollArea::vertical().id_salt("gw-output").max_height(170.0).stick_to_bottom(true).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text).monospace()).wrap());
        });
    }
    asked
}

fn drive_choice(ui: &mut Ui, prefs: &mut gw::Prefs) {
    egui::ComboBox::from_id_salt("gw-drive").selected_text(prefs.drive.clone()).width(60.0).show_ui(ui, |ui| {
        for drive in gw::DRIVES {
            ui.selectable_value(&mut prefs.drive, drive.to_string(), drive);
        }
    });
}

fn draw_setup(ui: &mut Ui, window: &mut GwWindow, prefs: &mut gw::Prefs) -> Option<Asked> {
    let mut asked = None;
    let installing = window.install.is_some();
    ui.label(crate::i18n::tr("Greaseweazle is a USB floppy disk controller (keirf's, on GitHub). Limoniter drives it through its own gw program."));
    ui.hyperlink_to(gw::HOME, gw::HOME);
    ui.add_space(4.0);
    let tool = window.info.as_ref().and_then(|i| i.tool.clone());
    match &window.program {
        Some(program) => {
            let version = tool.clone().map(|v| crate::i18n::tf!(" (version {v})")).unwrap_or_default();
            ui.label(crate::i18n::tf!("gw is found: {}{version}", program.display()));
        }
        None => {
            ui.label(RichText::new(crate::i18n::tr("gw isn't installed.")).strong());
            if cfg!(windows) {
                ui.label(crate::i18n::tr("Install downloads the latest release (a zip with gw.exe in it) from GitHub into Limoniter's own folder."));
            } else {
                ui.label(crate::i18n::tr("Install makes a Python environment of Limoniter's own and installs the tools into it from GitHub, as Greaseweazle's README says. It needs Python 3 and Git (and, to build one of its parts, a C compiler); if they're missing, it says so."));
            }
        }
    }
    if let (Some(latest), Some(tool)) = (&window.latest_version, &tool) {
        if gw::is_newer(latest, tool) {
            ui.label(crate::i18n::tf!("A newer version is out: {latest}."));
        } else {
            ui.label(crate::i18n::tf!("{latest} is the newest version, so this one is up to date."));
        }
    } else if let Some(latest) = &window.latest_version {
        ui.label(crate::i18n::tf!("The newest version is {latest}."));
    }
    let own = window.program.as_deref().is_some_and(gw::is_own);
    ui.horizontal_wrapped(|ui| {
        let label = if own { crate::i18n::tr("Update") } else { crate::i18n::tr("Install") };
        if (window.program.is_none() || own) && ui.add_enabled(!installing && !window.running(), egui::Button::new(label)).clicked() {
            asked = Some(Asked::Install);
        }
        if ui.add_enabled(window.latest.is_none(), egui::Button::new(crate::i18n::tr("Look for a newer version"))).clicked() {
            asked = Some(Asked::CheckLatest);
        }
        if ui.button(crate::i18n::tr("Choose gw\u{2026}")).clicked() {
            asked = Some(Asked::ChooseProgram);
        }
        if prefs.program.is_some() && ui.button(crate::i18n::tr("Forget the one chosen")).clicked() {
            asked = Some(Asked::ForgetProgram);
        }
        if installing {
            ui.spinner();
        }
    });
    if window.program.is_some() && !own && !installing {
        ui.label(RichText::new(crate::i18n::tr("This gw was installed outside Limoniter: update it the way it was installed.")).color(theme::text_faint()));
    }
    if !window.note.is_empty() {
        ui.add(egui::Label::new(window.note.clone()).wrap());
    }
    ui.separator();
    egui::Grid::new("gw-setup").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label(crate::i18n::tr("Drive"));
        drive_choice(ui, prefs);
        ui.end_row();
        ui.label(crate::i18n::tr("Port"));
        ui.add(egui::TextEdit::singleline(&mut prefs.port).hint_text(crate::i18n::tr("Empty: gw looks for it (COM3, /dev/ttyACM0)")).desired_width(260.0));
        ui.end_row();
    });
    ui.label(RichText::new(crate::i18n::tr("The drive is A or B for a PC cable, 0 to 3 for a Shugart one.")).color(theme::text_faint()));
    ui.horizontal(|ui| {
        if ui.add_enabled(window.program.is_some() && !window.running(), egui::Button::new(crate::i18n::tr("Test the connection"))).clicked() {
            asked = Some(Asked::Test);
        }
        if let Some(info) = &window.info {
            ui.label(info.describe());
        }
    });
    if let Some(newer) = window.info.as_ref().and_then(|i| i.newer_firmware.clone()) {
        ui.horizontal_wrapped(|ui| {
            ui.label(crate::i18n::tf!("Firmware {newer} is available."));
            if ui.add_enabled(!window.running(), egui::Button::new(crate::i18n::tr("Update the firmware"))).clicked() {
                asked = Some(Asked::Do(Action::Update));
            }
        });
        ui.label(RichText::new(crate::i18n::tr("Older Greaseweazles need their update jumper fitted first: gw's words below say.")).color(theme::text_faint()));
    }
    asked
}

fn draw_read(ui: &mut Ui, window: &mut GwWindow, prefs: &mut gw::Prefs, running: bool) -> Option<Asked> {
    let mut asked = None;
    egui::Grid::new("gw-read").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label(crate::i18n::tr("Drive"));
        drive_choice(ui, prefs);
        ui.end_row();
        ui.label(crate::i18n::tr("Kind of disk"));
        ui.vertical(|ui| disk_choice(ui, window, prefs, "gw-read-disk"));
        ui.end_row();
        ui.label(crate::i18n::tr("Save as"));
        ui.vertical(|ui| {
            let raw = prefs.format.is_empty();
            if raw {
                prefs.kind = Kind::Scp;
            }
            for kind in Kind::ALL {
                ui.add_enabled_ui(!raw || kind == Kind::Scp, |ui| {
                    ui.radio_value(&mut prefs.kind, kind, crate::i18n::tr(kind.label()));
                });
            }
        });
        ui.end_row();
        ui.label(crate::i18n::tr("Tracks"));
        ui.add(egui::TextEdit::singleline(&mut prefs.tracks).hint_text(crate::i18n::tr("All the disk has, such as c=0-79:h=0-1")).desired_width(260.0));
        ui.end_row();
        ui.label(crate::i18n::tr("Revolutions"));
        ui.add(egui::TextEdit::singleline(&mut prefs.revs).hint_text(crate::i18n::tr("Empty: gw chooses (more for flux)")).desired_width(120.0));
        ui.end_row();
        ui.label(crate::i18n::tr("Save to"));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut window.save_to).desired_width(300.0));
            if ui.button(crate::i18n::tr("Choose\u{2026}")).clicked() {
                asked = Some(Asked::ChooseSaveTo);
            }
        });
        ui.end_row();
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.add_enabled(!running && !window.save_to.trim().is_empty(), egui::Button::new(crate::i18n::tr("Read the disk"))).clicked() {
            asked = Some(Asked::Do(Action::Read(PathBuf::new())));
        }
        ui.label(RichText::new(crate::i18n::tr("It opens in Limoniter when it's done.")).color(theme::text_faint()));
    });
    asked
}

fn draw_write(ui: &mut Ui, window: &mut GwWindow, prefs: &mut gw::Prefs, front: Option<&Path>, running: bool) -> Option<Asked> {
    let mut asked = None;
    egui::Grid::new("gw-write").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label(crate::i18n::tr("Drive"));
        drive_choice(ui, prefs);
        ui.end_row();
        ui.label(crate::i18n::tr("Image to write"));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut window.write_from).desired_width(300.0));
            if ui.button(crate::i18n::tr("Choose\u{2026}")).clicked() {
                asked = Some(Asked::ChooseWriteFrom);
            }
        });
        ui.end_row();
        ui.label("");
        match front {
            Some(path) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if ui.button(crate::i18n::tf!("Use the open file: {name}")).clicked() {
                    asked = Some(Asked::UseFront);
                }
            }
            None => {
                ui.label(RichText::new(crate::i18n::tr("The open image would be offered here.")).color(theme::text_faint()));
            }
        }
        ui.end_row();
        ui.label(crate::i18n::tr("Kind of disk"));
        ui.vertical(|ui| disk_choice(ui, window, prefs, "gw-write-disk"));
        ui.end_row();
        ui.label(crate::i18n::tr("Tracks"));
        ui.add(egui::TextEdit::singleline(&mut prefs.tracks).hint_text(crate::i18n::tr("All the image has, such as c=0-79:h=0-1")).desired_width(260.0));
        ui.end_row();
        ui.label(crate::i18n::tr("Check it"));
        ui.checkbox(&mut prefs.verify, crate::i18n::tr("Read each track back after writing it")).on_hover_text(crate::i18n::tr("Takes a little longer; a track that fails is written again"));
        ui.end_row();
    });
    ui.colored_label(theme::rainbow_mistake(), crate::i18n::tr("Writing replaces everything on the disk."));
    let ready = !running && !window.write_from.trim().is_empty() && window.confirm.is_none();
    if ui.add_enabled(ready, egui::Button::new(crate::i18n::tr("Write the disk\u{2026}"))).clicked() {
        asked = Some(Asked::Confirm(Action::Write(PathBuf::new())));
    }
    asked
}

fn draw_tools(ui: &mut Ui, window: &mut GwWindow, running: bool) -> Option<Asked> {
    let mut asked = None;
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(!running, egui::Button::new(crate::i18n::tr("Drive info"))).clicked() {
            asked = Some(Asked::Do(Action::Info));
        }
        if ui.add_enabled(!running, egui::Button::new(crate::i18n::tr("Measure the drive's speed"))).on_hover_text(crate::i18n::tr("Needs a disk in the drive: a good drive turns at 300 rpm (360 for some)")).clicked() {
            asked = Some(Asked::Do(Action::Rpm));
        }
        if ui.add_enabled(!running, egui::Button::new(crate::i18n::tr("Clean the heads\u{2026}"))).clicked() {
            asked = Some(Asked::Confirm(Action::Clean));
        }
        if ui.add_enabled(!running, egui::Button::new(crate::i18n::tr("Erase a disk\u{2026}"))).clicked() {
            asked = Some(Asked::Confirm(Action::Erase));
        }
    });
    if let Some(info) = &window.info {
        ui.label(info.describe());
    }
    asked
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::tests::{app_with, draw};
    use super::*;
    use crate::greaseweazle::tests::fake_gw;
    use crate::media::write::Blank;

    fn settle(app: &mut Limoniter, ctx: &Context) {
        let began = std::time::Instant::now();
        loop {
            draw(app, ctx, Vec::new());
            let window = app.gw.as_ref().unwrap();
            if window.job.is_none() && window.queue.is_empty() && window.install.is_none() {
                break;
            }
            assert!(began.elapsed() < Duration::from_secs(30), "gw never finished");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-gw-host-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_read_runs_follows_gw_and_opens_the_image() {
        let dir = temp("read");
        let image = dir.join("blank.adf");
        std::fs::write(&image, Blank::AmigaDd.make("FLOPPY")).unwrap();
        let gw = fake_gw(&dir.join("bin"), "badsector", Some(&image));
        let (mut app, ctx) = app_with("");
        app.gw_prefs.program = Some(gw.clone());
        app.gw_prefs.format = "amiga.amigados".into();
        app.open_greaseweazle(Tab::Read, None);
        settle(&mut app, &ctx);
        // On opening: gw asked who's there.
        assert_eq!(app.gw.as_ref().unwrap().info.as_ref().unwrap().model.as_deref(), Some("Greaseweazle F7 Lightning"));
        // Read, into a place whose ending is wrong: it gets the kind's own.
        let target = dir.join("copy.img");
        let mut window = app.gw.take().unwrap();
        window.save_to = target.display().to_string();
        app.gw_asked(&mut window, Asked::Do(Action::Read(PathBuf::new())), None);
        app.gw = Some(window);
        settle(&mut app, &ctx);
        let said = app.gw.as_ref().unwrap().summary.clone().unwrap();
        assert!(said.contains("Found 43 sectors of 44"), "{said}");
        assert!(said.contains("couldn't be read"), "{said}");
        let adf = dir.join("copy.adf");
        assert!(adf.is_file() && !target.exists());
        assert_eq!(std::fs::read(&adf).unwrap().len(), 901120);
        // It opened in the disk browser.
        assert!(app.docs[app.active].browser.is_some(), "the image should open in the disk browser");
        let log = std::fs::read_to_string(dir.join("bin/log")).unwrap();
        assert!(log.contains(&format!("read --drive A --format amiga.amigados {}", adf.display())), "{log}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_write_waits_for_a_yes_and_then_runs() {
        let dir = temp("write");
        let image = dir.join("disk.adf");
        std::fs::write(&image, Blank::AmigaDd.make("FLOPPY")).unwrap();
        let gw = fake_gw(&dir.join("bin"), "good", None);
        let (mut app, ctx) = app_with("");
        app.gw_prefs.program = Some(gw);
        app.gw_prefs.format = "amiga.amigados".into();
        assert!(app.open_path(&image));
        app.open_greaseweazle(Tab::Write, None);
        settle(&mut app, &ctx);
        // The open image is offered and filled in.
        assert_eq!(app.gw.as_ref().unwrap().write_from, image.display().to_string());
        let mut window = app.gw.take().unwrap();
        app.gw_asked(&mut window, Asked::Confirm(Action::Write(PathBuf::new())), None);
        assert!(matches!(window.confirm, Some(Action::Write(_))));
        assert!(window.queue.is_empty() && window.job.is_none(), "nothing runs before the yes");
        app.gw_asked(&mut window, Asked::Do(Action::Write(PathBuf::new())), None);
        app.gw = Some(window);
        settle(&mut app, &ctx);
        let window = app.gw.as_ref().unwrap();
        assert!(window.confirm.is_none());
        assert!(window.summary.as_deref().unwrap().contains("verified"), "{:?}", window.summary);
        let log = std::fs::read_to_string(dir.join("bin/log")).unwrap();
        assert!(log.contains(&format!("write --drive A --format amiga.amigados {}", image.display())), "{log}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_drive_jobs_and_failures_are_told() {
        let dir = temp("tools");
        let (mut app, ctx) = app_with("");
        app.gw_prefs.program = Some(fake_gw(&dir.join("good"), "good", None));
        app.open_greaseweazle(Tab::Tools, Some(Action::Rpm));
        settle(&mut app, &ctx);
        assert!(app.gw.as_ref().unwrap().summary.as_deref().unwrap().contains("rpm"));
        // The firmware offered, and the old and new said.
        let window = app.gw.as_ref().unwrap();
        assert_eq!(window.info.as_ref().unwrap().newer_firmware.as_deref(), Some("1.7"));
        // Clean and erase ask first.
        app.open_greaseweazle_asking(Action::Erase);
        assert_eq!(app.gw.as_ref().unwrap().confirm, Some(Action::Erase));
        // No Greaseweazle plugged in: the words say so.
        app.gw_prefs.program = Some(fake_gw(&dir.join("none"), "nodevice", None));
        app.gw = None;
        app.open_greaseweazle(Tab::Tools, Some(Action::Rpm));
        settle(&mut app, &ctx);
        assert!(app.gw.as_ref().unwrap().summary.as_deref().unwrap().contains("plugged in"));
        // Without gw at all the window says to install it, and runs nothing.
        app.gw_prefs.program = None;
        app.gw = None;
        let mut window = GwWindow::new(&app.gw_prefs, Tab::Read);
        window.program = None;
        window.queue.push_back(Action::Info);
        window.next(&app.gw_prefs);
        assert!(window.job.is_none() && window.queue.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_menus_grey_gw_out_until_it_is_installed() {
        let dir = temp("menus");
        let (mut app, _ctx) = app_with("");
        // A chosen program that isn't there doesn't count.
        app.gw_prefs.program = Some(dir.join("no-such-gw"));
        let found = crate::greaseweazle::find(&app.gw_prefs).is_some();
        assert_eq!(app.can(Command::GwRead), found);
        assert!(app.can(Command::GwSetup), "setting up is always possible");
        app.gw_prefs.program = Some(fake_gw(&dir.join("bin"), "good", None));
        for command in [Command::GwRead, Command::GwWrite, Command::GwInfo, Command::GwClean, Command::GwRpm, Command::GwErase] {
            assert!(app.can(command));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
