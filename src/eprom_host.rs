//! The EPROM programmer's window: minipro found (or where to get it), the programmer plugged in, a chip chosen
//! from minipro's own list, and the file in front written to it (fitted to the chip's size), verified, read
//! back into a new tab, blank-checked or erased, with minipro's words shown as it goes.

use super::*;
use crate::eprom::{self, Action, FileFormat, Job, Placement, Programmer};
use std::collections::VecDeque;

const TITLE: &str = "EPROM programmer (minipro)";

pub(super) struct EpromWindow {
    program: Option<PathBuf>,
    /// The programmer minipro found, as it names it: "TL866II+ 04.2.132 (0x284)".
    attached: Option<String>,
    chips: Vec<String>,
    filter: String,
    /// The chip's size in bytes, and whether minipro said so (rather than it being guessed from the name).
    size: Option<(u64, bool)>,
    job: Option<Job>,
    /// What runs after the job, in turn.
    queue: VecDeque<Action>,
    output: String,
    summary: Option<String>,
    fill_text: String,
}

/// Where the image written (or read) waits for minipro, in the temporary folder.
fn image_path(ending: &str) -> PathBuf {
    std::env::temp_dir().join(format!("limoniter-eprom-{}.{ending}", std::process::id()))
}

impl EpromWindow {
    fn new(prefs: &eprom::Prefs, program: Option<PathBuf>) -> EpromWindow {
        let mut window = EpromWindow {
            program,
            attached: None,
            chips: Vec::new(),
            filter: String::new(),
            size: eprom::size_from_name(&prefs.chip).map(|s| (s, false)),
            job: None,
            queue: VecDeque::new(),
            output: String::new(),
            summary: None,
            fill_text: format!("{:02X}", prefs.fill),
        };
        // What's plugged in, then its chips, then the size of the chip chosen last time.
        window.queue.extend([Action::Detect, Action::List(prefs.programmer)]);
        if !prefs.chip.trim().is_empty() {
            window.queue.push_back(Action::Info);
        }
        window
    }

    /// Starts the next action waiting, if nothing's running.
    fn next(&mut self, prefs: &eprom::Prefs) {
        if self.job.is_some() {
            return;
        }
        let Some(program) = self.program.clone() else {
            self.queue.clear();
            return;
        };
        // A chip's details need a chip.
        if prefs.chip.trim().is_empty() {
            self.queue.retain(|a| *a != Action::Info);
        }
        if let Some(mut action) = self.queue.pop_front() {
            // The list for the programmer plugged in, where that's what's chosen.
            if action == Action::List(Programmer::Attached)
                && let Some(found) = self.attached.as_deref().and_then(Programmer::from_found)
            {
                action = Action::List(found);
            }
            self.job = Some(Job::start(&program, prefs, action));
        }
    }

    /// Takes in a finished job's answer, and gives back what was done, whether it worked, and what minipro said.
    fn finished(&mut self) -> Option<(Action, bool, String)> {
        let job = self.job.as_ref()?;
        let done = job.done()?;
        let text = job.text();
        let action = job.action.clone();
        self.job = None;
        let succeeded = match done {
            Ok(ok) => ok,
            Err(problem) => {
                self.summary = Some(problem);
                self.queue.clear();
                return None;
            }
        };
        let said = eprom::summary(&action, succeeded, &text);
        match &action {
            Action::Detect => self.attached = if succeeded { eprom::found(&text) } else { None },
            Action::List(_) => {
                self.chips = eprom::read_list(&text);
                // A long list is the answer, not something to read.
                self.output = format!("{}\n", text.lines().next().unwrap_or_default());
                self.summary = Some(said);
                return Some((action, succeeded, text));
            }
            Action::Info => {
                if let Some(size) = eprom::size_from_info(&text) {
                    self.size = Some((size, true));
                }
            }
            _ => {}
        }
        self.output = text.clone();
        self.summary = Some(said);
        Some((action, succeeded, text))
    }
}

impl EpromWindow {
    /// Uses this minipro, and looks for the programmer and its chips again with it.
    pub(super) fn use_program(&mut self, program: PathBuf) {
        self.program = Some(program);
        self.queue = VecDeque::from([Action::Detect, Action::List(Programmer::Attached), Action::Info]);
    }

    /// The chip, chosen as if from the list.
    pub(super) fn choose_chip(&mut self, prefs: &mut eprom::Prefs, chip: &str) {
        prefs.chip = chip.to_string();
        self.size = eprom::size_from_name(chip).map(|s| (s, false));
    }

    /// Runs whatever is waiting, to its end, there and then: for a screenshot, which can't wait for frames.
    pub(super) fn settle_now(&mut self, prefs: &eprom::Prefs) {
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

impl Limoniter {
    pub(super) fn open_eprom(&mut self) {
        if self.eprom.is_none() {
            self.eprom = Some(Box::new(EpromWindow::new(&self.eprom_prefs, eprom::find())));
        }
    }

    /// The bytes to write: the file in front (the hex editor's bytes, or the text as it would be saved). Intel
    /// HEX and S-record files go as they are, for minipro to read; anything else is fitted to the chip's size.
    fn eprom_image(&self, window: &EpromWindow) -> Result<(PathBuf, Option<FileFormat>), String> {
        if self.no_file() {
            return Err(crate::i18n::tr("Open the ROM to write first: its file, or its bytes in the hex editor.").into());
        }
        let name = self.docs[self.active].path_or_name().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let bytes = self.document_bytes();
        let format = FileFormat::of(&name);
        let prefs = &self.eprom_prefs;
        let (bytes, ending) = match (format, window.size) {
            (Some(FileFormat::IntelHex), _) => (bytes, "hex"),
            (Some(FileFormat::Srecord), _) => (bytes, "srec"),
            (None, Some((size, _))) => (eprom::fit(&bytes, size as usize, prefs.fill, prefs.placement)?, "bin"),
            (None, None) => (bytes, "bin"),
        };
        let path = image_path(ending);
        std::fs::write(&path, bytes).map_err(|e| crate::i18n::tf!("Couldn't write to {}: {e}", path.display()))?;
        Ok((path, format))
    }

    pub(super) fn eprom_window(&mut self, ctx: &Context) {
        let Some(mut window) = self.eprom.take() else { return };
        let before = self.eprom_prefs.clone();
        if let Some((action, succeeded, _)) = window.finished()
            && let Action::Read(path) = action
        {
            if succeeded {
                match std::fs::read(&path) {
                    Ok(bytes) => {
                        let chip = self.eprom_prefs.chip.split('@').next().unwrap_or("chip").to_string();
                        self.open_bytes_as_new(&format!("{chip}.bin"), bytes);
                    }
                    Err(e) => window.summary = Some(e.to_string()),
                }
            }
            let _ = std::fs::remove_file(&path);
        }
        window.next(&self.eprom_prefs);
        let front = if self.no_file() {
            None
        } else {
            let name = self.docs[self.active].path_or_name().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            Some(name)
        };
        let mut asked = None;
        let mut open = true;
        let prefs = &mut self.eprom_prefs;
        egui::Window::new(crate::i18n::tr(TITLE)).open(&mut open).resizable(true).default_width(620.0).show(ctx, |ui| {
            asked = draw_eprom(ui, &mut window, prefs, front.as_deref());
        });
        if let Some(asked) = asked {
            let action = match asked {
                Asked::Again => {
                    window.program = eprom::find();
                    window.queue.extend([Action::Detect, Action::List(self.eprom_prefs.programmer)]);
                    None
                }
                Asked::Do(action) => Some(action),
                Asked::Stop => {
                    if let Some(job) = &window.job {
                        job.stop();
                    }
                    window.queue.clear();
                    None
                }
            };
            let action = match action {
                Some(Action::Write(..)) => self.eprom_image(&window).map(|(path, format)| Action::Write(path, format)),
                Some(Action::Verify(..)) => self.eprom_image(&window).map(|(path, format)| Action::Verify(path, format)),
                Some(Action::Read(_)) => Ok(Action::Read(image_path("read.bin"))),
                Some(other) => Ok(other),
                None => Err(String::new()),
            };
            match action {
                Ok(action) => {
                    window.output.clear();
                    window.summary = None;
                    window.queue.push_back(action);
                    window.next(&self.eprom_prefs);
                }
                Err(problem) if !problem.is_empty() => window.summary = Some(problem),
                Err(_) => {}
            }
        }
        if window.job.is_some() || !window.queue.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if self.eprom_prefs != before {
            self.save_settings_if_changed();
        }
        if open {
            self.eprom = Some(window);
        } else if let Some(job) = &window.job {
            job.stop();
        }
    }
}

enum Asked {
    /// Look for minipro and the programmer again.
    Again,
    Do(Action),
    Stop,
}

fn draw_eprom(ui: &mut Ui, window: &mut EpromWindow, prefs: &mut eprom::Prefs, front: Option<&str>) -> Option<Asked> {
    let mut asked = None;
    match &window.program {
        Some(program) => {
            ui.horizontal_wrapped(|ui| {
                ui.label(crate::i18n::tf!("minipro: {}", program.display()));
                match &window.attached {
                    Some(found) => ui.label(crate::i18n::tf!("Programmer: {found}")),
                    None => ui.label(crate::i18n::tr("No programmer found yet.")),
                };
                if ui.button(crate::i18n::tr("Look again")).clicked() {
                    asked = Some(Asked::Again);
                }
            });
        }
        None => {
            ui.label(eprom::how_to_get());
            ui.horizontal(|ui| {
                ui.hyperlink_to(eprom::HOME, eprom::HOME);
                if ui.button(crate::i18n::tr("Look again")).clicked() {
                    asked = Some(Asked::Again);
                }
            });
            return asked;
        }
    }
    ui.separator();
    let running = window.job.is_some();
    egui::Grid::new("eprom-grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label(crate::i18n::tr("Chips for"));
        let before = prefs.programmer;
        egui::ComboBox::from_id_salt("eprom-programmer").selected_text(crate::i18n::tr(prefs.programmer.label())).width(200.0).show_ui(ui, |ui| {
            for programmer in Programmer::ALL {
                ui.selectable_value(&mut prefs.programmer, programmer, crate::i18n::tr(programmer.label()));
            }
        });
        if prefs.programmer != before {
            window.queue.push_back(Action::List(prefs.programmer));
        }
        ui.end_row();
        ui.label(crate::i18n::tr("Chip"));
        ui.vertical(|ui| {
            ui.add(egui::TextEdit::singleline(&mut window.filter).hint_text(crate::i18n::tr("Type part of its name: 27C256, AT28C64")).desired_width(260.0));
            let wanted = window.filter.trim().to_ascii_uppercase();
            let matching: Vec<&String> = window.chips.iter().filter(|c| wanted.is_empty() || c.to_ascii_uppercase().contains(&wanted)).collect();
            let mut chosen = None;
            egui::ScrollArea::vertical().id_salt("eprom-chips").min_scrolled_height(140.0).max_height(140.0).show(ui, |ui| {
                ui.set_min_width(260.0);
                for chip in matching.iter().take(300) {
                    if ui.selectable_label(**chip == prefs.chip, chip.as_str()).clicked() {
                        chosen = Some((*chip).clone());
                    }
                }
                if matching.len() > 300 {
                    let more = matching.len() - 300;
                    ui.label(crate::i18n::tf!("And {more} more: type more of the name."));
                }
                if window.chips.is_empty() && !running {
                    ui.label(crate::i18n::tr("minipro's list of chips isn't here: is a programmer plugged in?"));
                }
            });
            if let Some(chip) = chosen {
                prefs.chip = chip;
                window.size = eprom::size_from_name(&prefs.chip).map(|s| (s, false));
                window.queue.push_back(Action::Info);
            }
        });
        ui.end_row();
        ui.label(crate::i18n::tr("Chosen"));
        match (prefs.chip.trim().is_empty(), window.size) {
            (true, _) => ui.label(crate::i18n::tr("None yet")),
            (false, Some((size, known))) => {
                let chip = &prefs.chip;
                if known { ui.label(crate::i18n::tf!("{chip}: {size} bytes")) } else { ui.label(crate::i18n::tf!("{chip}: {size} bytes, going by its name")) }
            }
            (false, None) => ui.label(prefs.chip.clone()),
        };
        ui.end_row();
        ui.label(crate::i18n::tr("To write"));
        match front {
            Some(name) => ui.label(crate::i18n::tf!("The file in front: {name}")),
            None => ui.label(crate::i18n::tr("Open the ROM to write first: its file, or its bytes in the hex editor.")),
        };
        ui.end_row();
        ui.label(crate::i18n::tr("A smaller file"));
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("eprom-placement").selected_text(crate::i18n::tr(prefs.placement.label())).width(190.0).show_ui(ui, |ui| {
                for placement in Placement::ALL {
                    ui.selectable_value(&mut prefs.placement, placement, crate::i18n::tr(placement.label()));
                }
            });
            ui.label(crate::i18n::tr("the rest filled with $"));
            if ui.add(egui::TextEdit::singleline(&mut window.fill_text).char_limit(2).font(egui::TextStyle::Monospace).desired_width(28.0)).changed()
                && let Ok(fill) = u8::from_str_radix(window.fill_text.trim(), 16)
            {
                prefs.fill = fill;
            }
        });
        ui.end_row();
        ui.label(crate::i18n::tr("Options"));
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut prefs.ignore_id, crate::i18n::tr("Ignore the chip's ID")).on_hover_text(crate::i18n::tr("For a chip whose ID doesn't match, such as another maker's copy"));
            ui.checkbox(&mut prefs.skip_erase, crate::i18n::tr("Don't erase first")).on_hover_text(crate::i18n::tr("For a chip already blank, or a UV EPROM, which only a UV eraser can erase"));
            ui.checkbox(&mut prefs.icsp, crate::i18n::tr("In circuit (ICSP)"));
        });
        ui.end_row();
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        let ready = !running && !prefs.chip.trim().is_empty();
        let mut button = |ui: &mut Ui, label: &str, action: Action| {
            if ui.add_enabled(ready, egui::Button::new(label)).clicked() {
                asked = Some(Asked::Do(action));
            }
        };
        button(ui, crate::i18n::tr("Write and verify"), Action::Write(PathBuf::new(), None));
        button(ui, crate::i18n::tr("Verify"), Action::Verify(PathBuf::new(), None));
        button(ui, crate::i18n::tr("Read into a new tab"), Action::Read(PathBuf::new()));
        button(ui, crate::i18n::tr("Blank check"), Action::BlankCheck);
        button(ui, crate::i18n::tr("Erase"), Action::Erase);
        button(ui, crate::i18n::tr("Pin contact check"), Action::PinCheck);
        if running {
            ui.spinner();
            if ui.button(crate::i18n::tr("Stop")).clicked() {
                asked = Some(Asked::Stop);
            }
        }
    });
    if let Some(summary) = &window.summary {
        ui.label(RichText::new(summary).strong());
    }
    let text = window.job.as_ref().map_or_else(|| window.output.clone(), Job::text);
    if !text.is_empty() {
        egui::ScrollArea::vertical().id_salt("eprom-output").max_height(200.0).stick_to_bottom(true).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text).monospace()).wrap());
        });
    }
    asked
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::tests::{app_with, draw};
    use super::*;
    use std::time::Instant;

    fn settle(app: &mut Limoniter, ctx: &Context) {
        let began = Instant::now();
        loop {
            draw(app, ctx, Vec::new());
            let window = app.eprom.as_ref().unwrap();
            if window.job.is_none() && window.queue.is_empty() {
                break;
            }
            assert!(began.elapsed() < Duration::from_secs(30), "minipro never finished");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn the_window_lists_chips_and_writes_the_file_in_front() {
        let dir = std::env::temp_dir().join(format!("limoniter-eprom-window-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let program = crate::eprom::tests::fake_minipro(&dir.join("bin"));
        let rom = dir.join("cart.bin");
        std::fs::write(&rom, [0x09, 0x80, 0x09, 0x80, 0xC3, 0xC2, 0xCD, 0x38, 0x30]).unwrap();
        let (mut app, ctx) = app_with("");
        assert!(app.open_path(&rom));
        app.eprom_prefs.chip = "AT28C256".into();
        app.eprom_prefs.placement = Placement::Repeat;
        app.eprom = Some(Box::new(EpromWindow::new(&app.eprom_prefs, Some(program))));
        settle(&mut app, &ctx);
        // On opening: the programmer found, its list (the T48's, as that's what's plugged in), the chip's size.
        let window = app.eprom.as_ref().unwrap();
        assert_eq!(window.attached.as_deref(), Some("T48 01.1.31 (0x11f)"));
        assert_eq!(window.chips.len(), 7);
        assert_eq!(window.size, Some((32768, true)));
        let arguments = std::fs::read_to_string(dir.join("bin").join("arguments")).unwrap();
        assert_eq!(arguments.lines().collect::<Vec<_>>(), ["-k", "-q T48 -l", "-d AT28C256"]);
        // Written: the 9-byte cartridge header repeated through the chip's 32K.
        let mut window = app.eprom.take().unwrap();
        let (image, format) = app.eprom_image(&window).unwrap();
        assert_eq!(format, None);
        window.queue.push_back(Action::Write(image, format));
        app.eprom = Some(window);
        settle(&mut app, &ctx);
        let chip = std::fs::read(dir.join("bin").join("chip.bin")).unwrap();
        assert_eq!(chip.len(), 32768);
        assert_eq!(&chip[..9], &chip[9..18]);
        assert_eq!(chip[32767], [0x09, 0x80, 0x09, 0x80, 0xC3, 0xC2, 0xCD, 0x38, 0x30][32767 % 9]);
        assert!(app.eprom.as_ref().unwrap().summary.as_deref().unwrap().contains("verified"));
        // Read back into a new tab, named for the chip.
        app.eprom.as_mut().unwrap().queue.push_back(Action::Read(image_path("read.bin")));
        settle(&mut app, &ctx);
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(app.document_bytes(), chip);
        assert!(app.docs[app.active].path_or_name().display().to_string().contains("AT28C256.bin"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_minipro_it_says_where_to_get_it() {
        let (mut app, ctx) = app_with("");
        app.eprom = Some(Box::new(EpromWindow::new(&app.eprom_prefs, None)));
        draw(&mut app, &ctx, Vec::new());
        let window = app.eprom.as_ref().unwrap();
        assert!(window.job.is_none() && window.queue.is_empty());
        assert!(eprom::how_to_get().contains(eprom::HOME));
    }
}
