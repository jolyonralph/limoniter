//! The window's side of the tile map and level editor: its window, kept open beside the text, and what it asks for:
//! files opened and saved (its own .ltm, or CharPad's .ctm), the source at the cursor, and binary files.

use super::*;
use crate::tile_map::Project;
use crate::tile_map_editor::{Asked, Editor};

const TITLE: &str = "Tile map and level editor";

impl Limoniter {
    /// Opens the editor: with the tile map or CharPad file in front in it, if it's one, and writing in the form the
    /// source in front is in.
    pub(super) fn open_tile_map(&mut self) {
        if self.tile_map.is_some() {
            return;
        }
        let mut editor = Editor::default();
        if !self.no_file() {
            let path = self.docs[self.active].path_or_name();
            let ending = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ending == "ctm" || ending == "ltm" {
                if let Err(problem) = load_tile_map(&mut editor, &path, &self.document_bytes()) {
                    self.message(TITLE, problem);
                }
            } else if let Some(form) = crate::table_gen::Form::for_language(&self.docs[self.active].language_name()) {
                editor.form = form;
            }
        }
        self.tile_map = Some(Box::new(editor));
    }

    pub(super) fn tile_map_window(&mut self, ctx: &Context) {
        let Some(mut editor) = self.tile_map.take() else {
            return;
        };
        let mut open = true;
        let mut asked = None;
        crate::fit::Dialog::new(crate::i18n::tr(TITLE)).open(&mut open).resizable(false).default_pos([60.0, 40.0]).show(ctx, |ui| {
            asked = editor.draw(ui);
        });
        if !open || asked == Some(Asked::Close) {
            return;
        }
        match asked {
            Some(Asked::Insert(text)) => self.insert_from_tool(TITLE, &text),
            Some(other) => {
                if let Err(problem) = tile_map_asked(&mut editor, other) {
                    self.message(TITLE, problem);
                }
            }
            None => {}
        }
        self.tile_map = Some(editor);
    }
}

/// A file read into the editor by its ending: CharPad's .ctm, or the editor's own text.
pub(super) fn load_tile_map(editor: &mut Editor, path: &Path, bytes: &[u8]) -> Result<(), String> {
    let ending = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let project = if ending == "ctm" || bytes.starts_with(b"CTM") {
        let (project, version) = crate::tile_ctm::read(bytes)?;
        editor.ctm_version = version;
        project
    } else {
        Project::from_text(&String::from_utf8_lossy(bytes))?
    };
    editor.set_project(project);
    editor.path = Some(path.to_path_buf());
    if let Some(stem) = path.file_stem() {
        editor.label = crate::quantise::identifier(&stem.to_string_lossy().to_lowercase());
    }
    editor.note = crate::i18n::tf!("Opened {}.", path.display());
    Ok(())
}

/// The project saved by the ending of where it goes: a CharPad file of the version chosen, or the editor's own.
/// Says what CharPad's file had to leave out.
pub(super) fn save_tile_map(editor: &mut Editor, path: &Path) -> Result<(), String> {
    let ending = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let (bytes, notes) = if ending == "ctm" { crate::tile_ctm::write(&editor.project, editor.ctm_version)? } else { (editor.project.to_text().into_bytes(), Vec::new()) };
    codec::write_file(path, &bytes).map_err(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()))?;
    editor.path = Some(path.to_path_buf());
    editor.note = if notes.is_empty() { crate::i18n::tf!("Saved {}.", path.display()) } else { notes.join(" ") };
    Ok(())
}

fn tile_map_asked(editor: &mut Editor, asked: Asked) -> Result<(), String> {
    match asked {
        Asked::Open => {
            if let Some(path) = rfd::FileDialog::new()
                .set_title(crate::i18n::tr("Open a tile map"))
                .add_filter(crate::i18n::tr("Tile maps and CharPad projects"), &["ltm", "ctm"])
                .add_filter(crate::i18n::tr("Any file"), &["*"])
                .pick_file()
            {
                let bytes = std::fs::read(&path).map_err(|e| crate::i18n::tf!("Couldn't read {}.\n\n{e}", path.display()))?;
                load_tile_map(editor, &path, &bytes).map_err(|problem| crate::i18n::tf!("Couldn't open {}.\n\n{problem}", path.display()))?;
            }
        }
        Asked::Save if editor.path.is_some() => {
            let path = editor.path.clone().unwrap_or_default();
            save_tile_map(editor, &path)?;
        }
        Asked::Save | Asked::SaveAs => {
            let ctm = editor.project.mode.c64();
            let mut dialog = rfd::FileDialog::new().set_title(crate::i18n::tr("Save the tile map")).set_file_name(format!("{}.ltm", editor.label)).add_filter(crate::i18n::tr("Tile map"), &["ltm"]);
            if ctm {
                dialog = dialog.add_filter(crate::i18n::tr("CharPad project"), &["ctm"]);
            }
            if let Some(path) = dialog.save_file() {
                save_tile_map(editor, &path)?;
            }
        }
        Asked::SaveBinaries => {
            if let Some(folder) = rfd::FileDialog::new().set_title(crate::i18n::tr("Save the binary files into a folder")).pick_folder() {
                let files = editor.project.binaries(&editor.label, &editor.parts, editor.big_endian);
                for (name, bytes) in &files {
                    let path = folder.join(name);
                    codec::write_file(&path, bytes).map_err(|e| crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()))?;
                }
                editor.note = crate::i18n::tf!("Saved {} files in {}.", files.len(), folder.display());
            }
        }
        Asked::Insert(_) | Asked::Close => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    #[test]
    fn the_window_opens_and_draws() {
        let (mut app, ctx) = app_with("");
        app.open_tile_map();
        assert!(app.tile_map.is_some());
        for _ in 0..3 {
            ctx.run_ui(Default::default(), |ui| app.tile_map_window(ui.ctx())).textures_delta.clear();
        }
        assert!(app.tile_map.is_some());
    }

    #[test]
    fn files_are_saved_and_opened_by_their_ending() {
        let folder = std::env::temp_dir().join(format!("limoniter-tile-map-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut editor = Editor::new(crate::tile_map::example());
        for (name, version) in [("level.ltm", 9), ("level.ctm", 9), ("old.ctm", 7), ("older.ctm", 5)] {
            editor.ctm_version = version;
            let path = folder.join(name);
            save_tile_map(&mut editor, &path).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(bytes.starts_with(b"CTM"), name.ends_with(".ctm"), "{name}");
            let mut back = Editor::default();
            load_tile_map(&mut back, &path, &bytes).unwrap();
            assert_eq!(back.project.layers[0].cells, editor.project.layers[0].cells, "{name}");
            assert_eq!(back.project.tiles.iter().map(|t| t.cells.clone()).collect::<Vec<_>>(), editor.project.tiles.iter().map(|t| t.cells.clone()).collect::<Vec<_>>());
            assert_eq!(back.label, name.trim_end_matches(".ltm").trim_end_matches(".ctm"));
            assert_eq!(back.path.as_deref(), Some(path.as_path()));
            if name.ends_with(".ltm") {
                assert_eq!(back.project, editor.project);
            } else {
                assert_eq!(back.ctm_version, version);
            }
        }
        // Version 5 can't hold tile names: it says so.
        assert!(!editor.note.starts_with(&crate::i18n::tr("Saved").to_string()), "{}", editor.note);
        // The file in front opens in the editor.
        let (mut app, _ctx) = app_with("");
        assert!(app.open_path(&folder.join("level.ctm")));
        app.open_tile_map();
        assert_eq!(app.tile_map.as_ref().unwrap().project.map_size, (32, 12));
        // A Spectrum project can't go into a CharPad file.
        let mut spectrum = Editor::new(Project::new(crate::tile_map::Mode::Spectrum, (1, 1), (4, 4)));
        assert!(save_tile_map(&mut spectrum, &folder.join("zx.ctm")).is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
