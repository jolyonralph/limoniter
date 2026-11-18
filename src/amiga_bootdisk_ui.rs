//! The window for an Amiga disk with no file system: the disk's map of sectors, the pieces put on it, and the
//! boot block that loads them, with how much of its 1024 bytes it uses.

use crate::amiga_bootdisk::{self as disk, Density, Made, Owner, Piece, Plan};
use eframe::egui::{self, Color32, RichText, Sense, Ui, vec2};
use std::path::{Path, PathBuf};

/// What the window asks the editor to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Asked {
    Close,
    NewPlan,
    OpenPlan,
    SavePlan,
    AddFile,
    AddFront,
    ChooseFile(usize),
    OpenPiece(usize),
    EditBoot,
    ChooseBoot,
    UseExampleBoot,
    WriteOver,
    Write,
    Run,
}

pub struct BootDisk {
    pub plan: Plan,
    /// The plan's own file, once it has one.
    pub path: Option<PathBuf>,
    /// An image to write over, and which of its sectors already hold something.
    pub base: Option<(PathBuf, Vec<u8>)>,
    /// Each piece made, or what stopped it.
    pub made: Vec<Result<Made, String>>,
    /// The boot block's size, or what's wrong with it.
    pub boot: Result<usize, String>,
    pub selected: Option<usize>,
    pub note: Option<String>,
    /// Whether the pieces need making again before they're shown.
    pub stale: bool,
    /// The versions of the pieces' open documents last made from.
    pub seen: u64,
}

impl Default for BootDisk {
    fn default() -> Self {
        BootDisk { plan: Plan::default(), path: None, base: None, made: Vec::new(), boot: Ok(0), selected: None, note: None, stale: true, seen: 0 }
    }
}

/// A colour for each piece, from the theme's rainbow.
fn colour(piece: usize) -> Color32 {
    let rainbow = crate::theme::rainbow();
    rainbow[piece % rainbow.len()]
}

impl BootDisk {
    /// The pieces made again, from their files as they are now (open ones as they are in the editor).
    pub fn refresh(&mut self, read: &dyn Fn(&Path) -> Result<Vec<u8>, String>, boot_source: &str) {
        self.made = self.plan.pieces.iter().map(|p| disk::make_piece(p, read)).collect();
        let made = self.made_list();
        self.boot = match disk::make_boot(boot_source, &self.plan, &made) {
            Ok(_) => Ok(disk::boot_size(boot_source, &self.plan, &made).unwrap_or(disk::BOOT_BYTES)),
            Err(problem) => Err(problem),
        };
        self.stale = false;
    }

    pub fn made_list(&self) -> Vec<Option<Made>> {
        self.made.iter().map(|m| m.as_ref().ok().cloned()).collect()
    }

    pub fn owners(&self) -> Vec<Owner> {
        disk::map(&self.plan, &self.made_list())
    }

    /// Adds a piece for a file, at the first free sectors after the others, loaded after the last one loaded.
    pub fn add(&mut self, file: PathBuf, read: &dyn Fn(&Path) -> Result<Vec<u8>, String>) {
        let name = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut piece = Piece { name, file, sector: 2, load: 0x40000, start: self.plan.pieces.is_empty(), loaded: true };
        let made = disk::make_piece(&piece, read);
        let count = made.as_ref().map_or(1, Made::sectors);
        let after = self.plan.pieces.iter().zip(&self.made).filter_map(|(p, m)| m.as_ref().ok().map(|m| (p, m))).map(|(_, m)| (m.load + (m.bytes.len() as u32).div_ceil(0x1000) * 0x1000).max(0x40000)).max();
        piece.load = after.unwrap_or(0x40000);
        piece.sector = self.plan.next_free(&self.owners(), count).unwrap_or(2);
        self.plan.pieces.push(piece);
        self.selected = Some(self.plan.pieces.len() - 1);
        self.stale = true;
    }

    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        ui.horizontal_wrapped(|ui| {
            if ui.button(crate::i18n::tr("New")).clicked() {
                asked = Some(Asked::NewPlan);
            }
            if ui.button(crate::i18n::tr("Open a plan\u{2026}")).on_hover_text(crate::i18n::tr("A .bootdisk file: the pieces, their sectors and addresses, and the boot block")).clicked() {
                asked = Some(Asked::OpenPlan);
            }
            if ui.button(crate::i18n::tr("Save the plan\u{2026}")).clicked() {
                asked = Some(Asked::SavePlan);
            }
            ui.separator();
            let before = self.plan.density;
            egui::ComboBox::from_id_salt("bootdisk-density").selected_text(crate::i18n::tr(self.plan.density.name())).show_ui(ui, |ui| {
                for d in [Density::Double, Density::High] {
                    ui.selectable_value(&mut self.plan.density, d, crate::i18n::tr(d.name()));
                }
            });
            if before != self.plan.density {
                self.stale = true;
            }
        });
        if let Some(path) = &self.path {
            ui.label(RichText::new(path.display().to_string()).size(11.5).color(crate::theme::text_faint()));
        }
        ui.add_space(4.0);
        self.map(ui);
        ui.add_space(6.0);
        ui.label(RichText::new(crate::i18n::tr("Pieces")).strong());
        self.pieces(ui, &mut asked);
        ui.horizontal(|ui| {
            if ui.button(crate::i18n::tr("Add a file\u{2026}")).on_hover_text(crate::i18n::tr("Assembler source is assembled to its address; any other file goes on the disk as it is")).clicked() {
                asked = Some(Asked::AddFile);
            }
            if ui.button(crate::i18n::tr("Add the file in front")).clicked() {
                asked = Some(Asked::AddFront);
            }
        });
        ui.add_space(6.0);
        self.boot_block(ui, &mut asked);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let problems = disk::place_problems(&self.plan, &self.made_list());
            let ready = !self.plan.pieces.is_empty() && problems.is_empty() && self.boot.is_ok() && self.made.iter().all(Result::is_ok);
            if ui.add_enabled(ready, egui::Button::new(crate::i18n::tr("Write the disk\u{2026}"))).on_hover_text(crate::i18n::tr("Save it as an .adf, with the boot block and every piece at its sectors")).clicked() {
                asked = Some(Asked::Write);
            }
            if ui.add_enabled(ready, egui::Button::new(crate::i18n::tr("Run it"))).on_hover_text(crate::i18n::tr("Boot it on the Amiga emulator in File, Devices")).clicked() {
                asked = Some(Asked::Run);
            }
            match &self.base {
                Some((path, _)) => {
                    ui.label(RichText::new(crate::i18n::tf!("Over {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())).color(crate::theme::text_dim()));
                    if ui.small_button(crate::i18n::tr("A blank disk instead")).clicked() {
                        self.base = None;
                    }
                }
                None => {
                    if ui.button(crate::i18n::tr("Write over a disk image\u{2026}")).on_hover_text(crate::i18n::tr("Start from an .adf and put the pieces on it: what's already in its other sectors is kept")).clicked() {
                        asked = Some(Asked::WriteOver);
                    }
                }
            }
        });
        for problem in disk::place_problems(&self.plan, &self.made_list()) {
            ui.label(RichText::new(problem).color(crate::theme::problem_mark()));
        }
        if let Some(note) = &self.note {
            ui.label(RichText::new(note).color(crate::theme::text_dim()));
        }
        asked
    }

    /// The disk's sectors, a column a track: each piece in its colour, the boot block, what an image to write
    /// over already holds, and what's free. Clicking a free sector puts the selected piece there.
    fn map(&mut self, ui: &mut Ui) {
        let owners = self.owners();
        let used = self.base.as_ref().map(|(_, image)| disk::used_in(image)).unwrap_or_default();
        let per = self.plan.density.sectors_a_track();
        let tracks = 160;
        let cell = if per == 11 { vec2(4.0, 5.0) } else { vec2(4.0, 3.0) };
        let gap = 1.0;
        let size = vec2(tracks as f32 * (cell.x + gap) + 4.0, per as f32 * (cell.y + gap));
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let painter = ui.painter_at(rect);
        let at = |sector: usize| {
            let (track, s) = (sector / per, sector % per);
            // A little gap between cylinders' two sides and the next cylinder.
            let x = rect.min.x + track as f32 * (cell.x + gap) + (track / 2) as f32 * 0.0;
            egui::Rect::from_min_size(egui::pos2(x, rect.min.y + s as f32 * (cell.y + gap)), cell)
        };
        for (sector, owner) in owners.iter().enumerate() {
            let fill = match owner {
                Owner::Boot => crate::theme::text(),
                Owner::Piece(i) => colour(*i),
                Owner::Clash => crate::theme::problem_mark(),
                Owner::Free if used.get(sector).copied().unwrap_or(false) => crate::theme::text_dim(),
                Owner::Free => crate::theme::text_faint().gamma_multiply(0.35),
            };
            painter.rect_filled(at(sector), 0.0, fill);
        }
        let hovered = response.hover_pos().and_then(|pos| {
            let track = ((pos.x - rect.min.x) / (cell.x + gap)) as usize;
            let s = ((pos.y - rect.min.y) / (cell.y + gap)) as usize;
            (track < tracks && s < per).then_some(track * per + s)
        });
        if let Some(sector) = hovered {
            let (cylinder, side, s) = disk::place(self.plan.density, sector);
            let whose = match owners[sector] {
                Owner::Boot => crate::i18n::tr("the boot block").to_string(),
                Owner::Piece(i) => self.plan.pieces[i].name.clone(),
                Owner::Clash => crate::i18n::tr("more than one piece").to_string(),
                Owner::Free if used.get(sector).copied().unwrap_or(false) => crate::i18n::tr("already used on the disk written over").to_string(),
                Owner::Free => crate::i18n::tr("free").to_string(),
            };
            response.clone().on_hover_text(crate::i18n::tf!("Sector {sector} (cylinder {cylinder}, side {side}, sector {s}): {whose}"));
            if response.clicked()
                && let Some(i) = self.selected
                && owners[sector] == Owner::Free
            {
                self.plan.pieces[i].sector = sector;
                self.stale = true;
            }
        }
        let free = owners.iter().enumerate().filter(|(s, o)| **o == Owner::Free && !used.get(*s).copied().unwrap_or(false)).count();
        ui.label(RichText::new(crate::i18n::tf!("{free} of {} sectors free ({} KB). A column is a track; the boot block is the first two sectors. Click a free sector to put the selected piece there.", owners.len(), free / 2)).size(11.5).color(crate::theme::text_dim()));
    }

    fn pieces(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        let mut remove = None;
        let mut start = None;
        egui::Grid::new("bootdisk-pieces").striped(true).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
            for title in ["", "Name", "File", "First sector", "Sectors", "Loaded to", "Start", "Loaded", ""] {
                ui.label(RichText::new(crate::i18n::tr(title)).size(11.5).color(crate::theme::text_dim()));
            }
            ui.end_row();
            for i in 0..self.plan.pieces.len() {
                let made = self.made.get(i);
                let selected = self.selected == Some(i);
                let (swatch, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                ui.painter().rect_filled(swatch, 2.0, colour(i));
                let piece = &mut self.plan.pieces[i];
                if ui.add_sized([120.0, 20.0], egui::TextEdit::singleline(&mut piece.name)).gained_focus() {
                    self.selected = Some(i);
                }
                ui.horizontal(|ui| {
                    let name = piece.file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    if ui.selectable_label(selected, name).on_hover_text(piece.file.display().to_string()).clicked() {
                        self.selected = Some(i);
                    }
                    if ui.small_button(crate::i18n::tr("\u{2026}")).on_hover_text(crate::i18n::tr("Choose another file")).clicked() {
                        *asked = Some(Asked::ChooseFile(i));
                    }
                    if ui.small_button(crate::i18n::tr("Open")).on_hover_text(crate::i18n::tr("Open the file to change it")).clicked() {
                        *asked = Some(Asked::OpenPiece(i));
                    }
                });
                if ui.add(egui::DragValue::new(&mut piece.sector).range(2..=self.plan.density.sectors() - 1)).changed() {
                    self.stale = true;
                }
                match made {
                    Some(Ok(m)) => ui.label(crate::i18n::tf!("{} ({} bytes)", m.sectors(), m.bytes.len())),
                    Some(Err(problem)) => ui.label(RichText::new(crate::i18n::tr("can't be made")).color(crate::theme::problem_mark())).on_hover_text(problem.clone()),
                    None => ui.label(""),
                };
                let own_org = disk::is_source(&piece.file) && made.and_then(|m| m.as_ref().ok()).is_some_and(|m| m.load != piece.load);
                if own_org {
                    let load = made.and_then(|m| m.as_ref().ok()).map_or(0, |m| m.load);
                    ui.label(crate::i18n::tf!("${load:X} (its ORG)"));
                } else {
                    let mut text = format!("${:X}", piece.load);
                    if ui.add_sized([86.0, 20.0], egui::TextEdit::singleline(&mut text).font(egui::TextStyle::Monospace)).changed()
                        && let Some(n) = disk::number(&text)
                    {
                        piece.load = n;
                        self.stale = true;
                    }
                }
                if ui.radio(piece.start, "").on_hover_text(crate::i18n::tr("The boot block jumps here once everything's loaded")).clicked() {
                    start = Some(i);
                }
                if ui.checkbox(&mut piece.loaded, "").on_hover_text(crate::i18n::tr("Whether the boot block loads it: leave data your program loads itself unticked")).changed() {
                    self.stale = true;
                }
                if ui.small_button(crate::i18n::tr("\u{D7}")).on_hover_text(crate::i18n::tr("Take it off the disk")).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = start {
            for (n, p) in self.plan.pieces.iter_mut().enumerate() {
                p.start = n == i;
            }
            self.stale = true;
        }
        if let Some(i) = remove {
            self.plan.pieces.remove(i);
            self.selected = None;
            self.stale = true;
        }
        if self.plan.pieces.is_empty() {
            ui.label(RichText::new(crate::i18n::tr("Nothing on the disk yet: add a program, assembled from its source, and any data it needs.")).color(crate::theme::text_faint()));
        }
    }

    fn boot_block(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        ui.label(RichText::new(crate::i18n::tr("Boot block")).strong());
        ui.horizontal_wrapped(|ui| {
            match &self.plan.boot {
                None => ui.label(crate::i18n::tr("Limoniter's example: it loads each piece the table lists and jumps to the one that starts.")),
                Some(path) => ui.label(crate::i18n::tf!("Your own: {}", path.display())),
            };
        });
        ui.horizontal(|ui| {
            let edit = if self.plan.boot.is_none() { crate::i18n::tr("Change it\u{2026}") } else { crate::i18n::tr("Open it") };
            let hint = if self.plan.boot.is_none() { crate::i18n::tr("Save the example's source beside the plan and open it, to change as you like: it's used from then on") } else { crate::i18n::tr("Open the boot block's source") };
            if ui.button(edit).on_hover_text(hint).clicked() {
                *asked = Some(Asked::EditBoot);
            }
            if ui.button(crate::i18n::tr("Use a source of your own\u{2026}")).clicked() {
                *asked = Some(Asked::ChooseBoot);
            }
            if self.plan.boot.is_some() && ui.button(crate::i18n::tr("Use Limoniter's example")).clicked() {
                *asked = Some(Asked::UseExampleBoot);
            }
        });
        match &self.boot {
            Ok(size) => {
                let fraction = *size as f32 / disk::BOOT_BYTES as f32;
                ui.add(egui::ProgressBar::new(fraction).desired_width(360.0).text(crate::i18n::tf!("{size} of 1024 bytes used, {} free", disk::BOOT_BYTES - size)));
            }
            Err(problem) => {
                ui.label(RichText::new(problem).color(crate::theme::problem_mark()));
            }
        }
    }
}
