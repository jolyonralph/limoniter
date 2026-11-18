//! The window's side of the boot disk tool: reading the pieces (from the editor where they're open, unsaved changes
//! and all), the plan's file, the boot block's source, and writing or running the disk.

use super::*;
use crate::amiga_bootdisk as disk;
use crate::amiga_bootdisk_ui::{Asked, BootDisk};

const TITLE: &str = "Amiga boot disk (no file system)";

impl Limoniter {
    pub(super) fn open_boot_disk(&mut self) {
        if self.boot_disk.is_none() {
            self.boot_disk = Some(Box::default());
        }
    }

    /// A file's bytes as the editor has it, if it's open (unsaved changes and all), else as it is on disk.
    fn read_for_disk(&self, path: &Path) -> Result<Vec<u8>, String> {
        if let Some(doc) = self.docs.iter().find(|d| d.path.as_deref().is_some_and(|p| same_path(p, path))) {
            return Ok(if doc.hex_mode { doc.hex.read(0, doc.hex.len()) } else { doc.text.encode(&doc.text.format, crate::codec::Lossy::Omit).unwrap_or_default() });
        }
        std::fs::read(path).map_err(|e| crate::i18n::tf!("Couldn't read {}: {e}", path.display()))
    }

    /// The boot block's source: the person's own, or Limoniter's example.
    fn boot_source(&self, plan: &disk::Plan) -> Result<String, String> {
        match &plan.boot {
            Some(path) => self.read_for_disk(path).map(|b| String::from_utf8_lossy(&b).into_owned()),
            None => Ok(disk::example_boot_source()),
        }
    }

    /// What the pieces' open documents are at, so a change in the editor makes them again.
    fn boot_disk_versions(&self, bd: &BootDisk) -> u64 {
        let files = bd.plan.pieces.iter().map(|p| p.file.as_path()).chain(bd.plan.boot.as_deref());
        files.filter_map(|f| self.docs.iter().find(|d| d.path.as_deref().is_some_and(|p| same_path(p, f)))).fold(0u64, |sum, d| sum.wrapping_mul(31).wrapping_add(d.text.version()).wrapping_add(d.id))
    }

    fn refresh_boot_disk(&self, bd: &mut BootDisk) {
        match self.boot_source(&bd.plan) {
            Ok(source) => bd.refresh(&|p| self.read_for_disk(p), &source),
            Err(problem) => {
                bd.made = bd.plan.pieces.iter().map(|p| disk::make_piece(p, &|f| self.read_for_disk(f))).collect();
                bd.boot = Err(problem);
                bd.stale = false;
            }
        }
    }

    pub(super) fn boot_disk_window(&mut self, ctx: &Context) {
        let Some(mut bd) = self.boot_disk.take() else {
            return;
        };
        let versions = self.boot_disk_versions(&bd);
        if bd.stale || bd.seen != versions {
            bd.seen = versions;
            self.refresh_boot_disk(&mut bd);
        }
        let mut open = true;
        let mut asked = None;
        egui::Window::new(crate::i18n::tr(TITLE)).open(&mut open).resizable(false).default_pos([80.0, 50.0]).show(ctx, |ui| {
            asked = bd.draw(ui);
        });
        if !open || asked == Some(Asked::Close) {
            return;
        }
        if let Some(asked) = asked {
            bd.note = None;
            if let Err(problem) = self.boot_disk_asked(&mut bd, asked) {
                self.message(TITLE, problem);
            }
        }
        if bd.stale {
            self.refresh_boot_disk(&mut bd);
        }
        self.boot_disk = Some(bd);
    }

    /// The disk made, from the plan as it is.
    fn build_boot_disk(&self, bd: &BootDisk) -> Result<disk::Built, String> {
        let source = self.boot_source(&bd.plan)?;
        disk::build(&bd.plan, &source, bd.base.as_ref().map(|(_, image)| image.as_slice()), &|p| self.read_for_disk(p))
    }

    fn boot_disk_asked(&mut self, bd: &mut BootDisk, asked: Asked) -> Result<(), String> {
        let folder = bd.path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        let picker = |title: &str| {
            let picker = rfd::FileDialog::new().set_title(crate::i18n::tr(title));
            match &folder {
                Some(f) => picker.set_directory(f),
                None => picker,
            }
        };
        match asked {
            Asked::Close => {}
            Asked::NewPlan => *bd = BootDisk::default(),
            Asked::OpenPlan => {
                let Some(path) = picker("Open a boot disk plan").add_filter(crate::i18n::tr("Boot disk plan"), &["bootdisk"]).pick_file() else { return Ok(()) };
                let text = std::fs::read_to_string(&path).map_err(|e| crate::i18n::tf!("Couldn't read {}.\n\n{e}", path.display()))?;
                let plan = disk::Plan::from_text(&text, path.parent())?;
                *bd = BootDisk { plan, path: Some(path), ..BootDisk::default() };
            }
            Asked::SavePlan => {
                let path = match &bd.path {
                    Some(path) => path.clone(),
                    None => {
                        let Some(path) = picker("Save the boot disk plan").set_file_name("disk.bootdisk").add_filter(crate::i18n::tr("Boot disk plan"), &["bootdisk"]).save_file() else { return Ok(()) };
                        path
                    }
                };
                codec::write_file(&path, bd.plan.to_text(path.parent()).as_bytes()).map_err(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()))?;
                bd.note = Some(crate::i18n::tf!("Saved the plan as {}.", path.display()));
                bd.path = Some(path);
            }
            Asked::AddFile => {
                if let Some(path) = picker("Put a file on the disk").pick_file() {
                    bd.add(path, &|p| self.read_for_disk(p));
                }
            }
            Asked::AddFront => {
                let doc = &self.docs[self.active];
                let path = doc.path.clone().filter(|_| !self.no_file()).ok_or(crate::i18n::tr("Save the file in front first, so the plan can keep where it is."))?;
                bd.add(path, &|p| self.read_for_disk(p));
            }
            Asked::ChooseFile(i) => {
                if let Some(path) = picker("Choose the piece's file").pick_file()
                    && let Some(piece) = bd.plan.pieces.get_mut(i)
                {
                    piece.file = path;
                    bd.stale = true;
                }
            }
            Asked::OpenPiece(i) => {
                if let Some(piece) = bd.plan.pieces.get(i) {
                    let file = piece.file.clone();
                    self.open_path(&file);
                }
            }
            Asked::EditBoot => match bd.plan.boot.clone() {
                Some(path) => {
                    self.open_path(&path);
                }
                None => {
                    let Some(path) = picker("Save the boot block's source").set_file_name("bootblock.s").add_filter(crate::i18n::tr("68000 source"), &["s", "asm"]).save_file() else { return Ok(()) };
                    let source = disk::with_table(&disk::example_boot_source(), &disk::table_source(&bd.plan, &bd.made_list()));
                    codec::write_file(&path, crate::i18n::comments(&source).as_bytes()).map_err(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()))?;
                    bd.plan.boot = Some(path.clone());
                    bd.stale = true;
                    self.open_path(&path);
                }
            },
            Asked::ChooseBoot => {
                if let Some(path) = picker("Choose the boot block's source").add_filter(crate::i18n::tr("68000 source"), &["s", "asm", "i"]).pick_file() {
                    bd.plan.boot = Some(path);
                    bd.stale = true;
                }
            }
            Asked::UseExampleBoot => {
                bd.plan.boot = None;
                bd.stale = true;
            }
            Asked::WriteOver => {
                let Some(path) = picker("Write over a disk image").add_filter(crate::i18n::tr("Amiga disk image"), &["adf"]).pick_file() else { return Ok(()) };
                let image = std::fs::read(&path).map_err(|e| crate::i18n::tf!("Couldn't read {}.\n\n{e}", path.display()))?;
                bd.plan.density = match image.len() {
                    901_120 => disk::Density::Double,
                    1_802_240 => disk::Density::High,
                    n => return Err(crate::i18n::tf!("{} is {n} bytes: an Amiga disk image is 901,120 (880K) or 1,802,240 (1760K).", path.display())),
                };
                bd.base = Some((path, image));
                bd.stale = true;
            }
            Asked::Write => {
                let built = self.build_boot_disk(bd)?;
                let stem = bd.path.as_deref().and_then(Path::file_stem).map_or_else(|| "disk".to_string(), |s| s.to_string_lossy().into_owned());
                let Some(path) = picker("Write the disk").set_file_name(format!("{stem}.adf")).add_filter(crate::i18n::tr("Amiga disk image"), &["adf"]).save_file() else { return Ok(()) };
                codec::write_file(&path, &built.image).map_err(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()))?;
                let used = built.made.iter().flatten().map(disk::Made::sectors).sum::<usize>() + 2;
                bd.note = Some(crate::i18n::tf!("Wrote {}: the boot block uses {} of its 1024 bytes, and {used} sectors hold something.", path.display(), built.boot_size));
            }
            Asked::Run => {
                let built = self.build_boot_disk(bd)?;
                let stem = bd.path.as_deref().and_then(Path::file_stem).map_or_else(|| "disk".to_string(), |s| s.to_string_lossy().into_owned());
                let run = crate::running::Runnable { machine: crate::running::Machine::Amiga, format: crate::running::Format::Adf, name: format!("{stem}.adf"), bytes: std::sync::Arc::new(built.image) };
                self.run_made(run);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::tests::app_with;

    #[test]
    fn the_window_builds_from_the_editor_s_own_text_and_runs_the_disk() {
        let (mut app, _ctx) = app_with("");
        let folder = std::env::temp_dir().join(format!("limoniter-bootdisk-host-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let source = folder.join("main.s");
        std::fs::write(&source, "\t\tmoveq\t#1,d0\n").unwrap();
        assert!(app.open_path(&source));
        // A change not saved yet is what goes on the disk.
        let index = app.active;
        app.docs[index].text.set_selection((0, 0), (0, 0), false);
        app.docs[index].text.type_text("loop:\tbra.s\tloop\n");
        app.open_boot_disk();
        let mut bd = app.boot_disk.take().unwrap();
        app.boot_disk_asked(&mut bd, Asked::AddFront).unwrap();
        app.refresh_boot_disk(&mut bd);
        assert_eq!(bd.plan.pieces.len(), 1);
        assert!(bd.plan.pieces[0].start && bd.plan.pieces[0].sector == 2 && bd.plan.pieces[0].load == 0x40000);
        let made = bd.made[0].as_ref().unwrap();
        assert_eq!(made.bytes, [0x60, 0xFE, 0x70, 0x01], "BRA.S to itself, then MOVEQ #1,D0");
        assert!(matches!(bd.boot, Ok(size) if size < 200));
        let built = app.build_boot_disk(&bd).unwrap();
        assert_eq!(&built.image[1024..1028], &[0x60, 0xFE, 0x70, 0x01]);
        // Run it: with no Amiga emulator set up, it asks where.
        app.boot_disk_asked(&mut bd, Asked::Run).unwrap();
        assert!(matches!(&app.dialog, Dialog::Run(boxed) if matches!(boxed.as_ref(), super::run_host::RunBox::Where { ready: true, .. })));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
