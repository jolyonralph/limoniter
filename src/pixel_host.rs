//! The window's side of the character and sprite editor: its window, kept open beside the text, and what it asks
//! for: files, the source at the cursor, the selection, and a Spectrum listing's UDG pictures.

use super::*;
use crate::pixel_editor::{Asked, Editor, Kind, Sheet};

/// A file read into the editor: SpritePad's .spd as C64 sprites, a .prg (or .64c) without its load address, .spr
/// as Next sprites unless C64 ones are being drawn, and anything else as the kind being drawn.
fn load_into(editor: &mut Editor, path: &Path, bytes: &[u8]) -> Result<(), String> {
    let ending = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let kind = editor.sheet.kind;
    let colours = editor.sheet.colours;
    if ending == "spd" {
        let sheet = Sheet::from_spd(bytes).ok_or_else(|| crate::i18n::tf!("{} isn't a SpritePad file Limoniter knows.", path.display()))?;
        *editor = Editor::new(sheet.kind);
        editor.sheet = sheet;
        return Ok(());
    }
    let (kind, bytes, load) = match ending.as_str() {
        "spr" if !kind.c64() => (Kind::Sprites, bytes, None),
        "prg" | "64c" if bytes.len() > 2 => {
            let kind = if kind.c64() { kind } else { Kind::C64Characters };
            (kind, &bytes[2..], Some(u16::from_le_bytes([bytes[0], bytes[1]])))
        }
        _ => (kind, bytes, None),
    };
    *editor = Editor::new(kind);
    editor.sheet = Sheet::from_bytes(kind, bytes);
    editor.sheet.colours = colours;
    if let Some(load) = load {
        editor.load_address = load;
    }
    Ok(())
}

impl Limoniter {
    pub(super) fn pixel_editor_window(&mut self, ctx: &Context) {
        let Some(mut editor) = self.pixel_editor.take() else {
            return;
        };
        let spectrum_basic = !self.no_file() && self.docs[self.active].text.zx.is_some();
        let mut open = true;
        let mut asked = None;
        crate::fit::Dialog::new(crate::i18n::tr("Characters and sprites")).open(&mut open).resizable(false).default_pos([200.0, 120.0]).show(ctx, |ui| {
            asked = editor.draw(ui, spectrum_basic);
        });
        if !open {
            asked = Some(Asked::Close);
        }
        let Some(asked) = asked else {
            self.pixel_editor = Some(editor);
            return;
        };
        match asked {
            Asked::Close => return,
            Asked::Load => {
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Open characters or sprites")).add_filter(crate::i18n::tr("UDGs, fonts and sprites"), &["udg", "chr", "ch8", "fnt", "spr", "bin", "prg", "64c", "spd"]).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                    match std::fs::read(&path) {
                        Ok(bytes) => {
                            if let Err(problem) = load_into(&mut editor, &path, &bytes) {
                                self.message("Characters and sprites", problem);
                            }
                        }
                        Err(e) => self.message("Characters and sprites", crate::i18n::tf!("Couldn't read {}.\n\n{e}", path.display())),
                    }
                }
            }
            Asked::Save => {
                let kind = editor.sheet.kind;
                let (filter, endings): (&str, &[&str]) = match kind {
                    Kind::Sprites => ("Next sprites", &["spr", "bin"]),
                    Kind::Characters => ("UDGs or a font", &["udg", "chr", "bin"]),
                    Kind::C64Sprites | Kind::C64MultiSprites => ("C64 sprites", &["bin", "prg", "spd"]),
                    _ => ("C64 characters", &["bin", "prg", "64c"]),
                };
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save the characters or sprites")).set_file_name(format!("{}.{}", editor.label, endings[0])).add_filter(filter, endings).save_file() {
                    let ending = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                    let bytes = match ending.as_str() {
                        "prg" | "64c" if kind.c64() => editor.prg(),
                        "spd" if kind.c64_sprite() => editor.sheet.to_spd(),
                        _ => editor.sheet.to_bytes(),
                    };
                    if let Err(e) = codec::write_file(&path, &bytes) {
                        self.message("Characters and sprites", crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()));
                    }
                }
            }
            Asked::Insert => {
                let text = editor.sheet.to_form(&editor.label, editor.output);
                if self.no_file() || self.docs[self.active].hex_mode || self.docs[self.active].text.read_only {
                    self.message("Characters and sprites", crate::i18n::tr("Open the source to put them in, with the cursor where they go.").into());
                } else {
                    let doc = &mut self.docs[self.active];
                    doc.text.begin_undo_group(false);
                    doc.text.type_text(&text);
                    doc.text.end_undo_group();
                }
            }
            Asked::FromSelection => {
                let kind = editor.sheet.kind;
                let selected = if self.no_file() { String::new() } else { self.docs[self.active].text.selection_text() };
                match Sheet::from_text(kind, &selected) {
                    Some(sheet) => {
                        editor.sheet = sheet;
                        editor.current = 0;
                    }
                    None => self.message("Characters and sprites", crate::i18n::tr("Select DB lines (DB, DEFB, !byte or FCB) whose numbers are the pictures.").into()),
                }
            }
            Asked::UseAsUdgs => {
                let bytes = editor.sheet.to_bytes();
                let index = self.active;
                self.give_udg_pictures(index, crate::zx_graphics::udgs_from_bytes(&bytes, 0), crate::i18n::tr("the character editor").into());
            }
        }
        self.pixel_editor = Some(editor);
    }
}

impl Limoniter {
    pub(super) fn screen_designer_window(&mut self, ctx: &Context) {
        use crate::screen_designer::{Asked, Screen};
        let Some(mut designer) = self.screen_designer.take() else {
            return;
        };
        let mut open = true;
        let mut asked = None;
        crate::fit::Dialog::new(crate::i18n::tr("Spectrum screen")).open(&mut open).resizable(false).default_pos([160.0, 80.0]).show(ctx, |ui| {
            asked = designer.draw(ui);
        });
        if !open {
            return;
        }
        match asked {
            Some(Asked::Close) => return,
            Some(Asked::Open) => {
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Open a Spectrum screen")).add_filter(crate::i18n::tr("Spectrum screens"), &["scr"]).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file() {
                    let read = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| Screen::from_scr(&bytes).ok_or_else(|| crate::i18n::tr("It isn't a Spectrum screen: those are 6912 bytes (or 6144 without colours).").to_string()));
                    match read {
                        Ok(screen) => {
                            designer.remember();
                            designer.screen = screen;
                        }
                        Err(problem) => self.message("Spectrum screen", crate::i18n::tf!("Couldn't open {}.\n\n{problem}", path.display())),
                    }
                }
            }
            Some(Asked::BringIn) => {
                self.screen_designer = Some(designer);
                self.bring_picture_to_designer();
                return;
            }
            Some(Asked::SaveTap) => {
                let scr = designer.screen.to_scr();
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save the screen as a tape")).set_file_name("screen.tap").add_filter(crate::i18n::tr("Spectrum tape"), &["tap"]).save_file() {
                    let name = path.file_stem().map_or_else(|| "screen".to_string(), |s| s.to_string_lossy().into_owned());
                    if let Err(e) = codec::write_file(&path, &crate::retro_convert::spectrum_tape(&name, &scr)) {
                        self.message("Spectrum screen", crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()));
                    }
                }
            }
            Some(Asked::Save) => {
                if let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save the screen")).set_file_name("screen.scr").add_filter(crate::i18n::tr("Spectrum screen"), &["scr"]).save_file()
                    && let Err(e) = codec::write_file(&path, &designer.screen.to_scr())
                {
                    self.message("Spectrum screen", crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()));
                }
            }
            Some(Asked::SavePng) => {
                let picture = crate::pictures::decode(crate::pictures::Format::SpectrumScreen, &designer.screen.to_scr());
                if let Ok(png) = picture.and_then(|p| crate::pictures::to_png(&p))
                    && let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save the screen as PNG")).set_file_name("screen.png").add_filter(crate::i18n::tr("PNG picture"), &["png"]).save_file()
                    && let Err(e) = codec::write_file(&path, &png)
                {
                    self.message("Spectrum screen", crate::i18n::tf!("Couldn't save {}.\n\n{e}", path.display()));
                }
            }
            None => {}
        }
        self.screen_designer = Some(designer);
    }
}

impl Limoniter {
    /// Shows or puts away the Terminal panel, its shells running on; with none yet, starts one.
    pub(super) fn toggle_terminal(&mut self) {
        if self.terminal.is_some() {
            self.terminal_hidden = !self.terminal_hidden;
            if self.terminal_hidden {
                self.pending_focus = true;
            }
            return;
        }
        self.new_terminal(None);
    }

    /// Starts another terminal, in front of any others: in a folder, or the open folder's, or the file's.
    pub(super) fn new_terminal(&mut self, folder: Option<PathBuf>) {
        let shell = crate::terminal::Shell::from_id(&self.terminal_shell);
        self.new_terminal_with(folder, &shell);
    }

    /// A terminal running a shell of the kind given (a WSL distribution, Command Prompt…).
    pub(super) fn new_terminal_with(&mut self, folder: Option<PathBuf>, shell: &crate::terminal::Shell) {
        let folder = folder
            .or_else(|| self.project.as_ref().and_then(|p| p.root.clone()))
            .or_else(|| self.docs.get(self.active).and_then(|d| d.path.as_deref()).and_then(Path::parent).map(Path::to_path_buf))
            .or_else(|| std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));
        match crate::terminal::Terminal::start_shell(shell, &folder, 12, 80) {
            Ok(terminal) => self.show_terminal(terminal),
            Err(problem) => self.message("Terminal", problem),
        }
    }

    /// Puts a terminal that's been started in front of any others.
    pub(super) fn show_terminal(&mut self, terminal: crate::terminal::Terminal) {
        if let Some(front) = self.terminal.take() {
            self.terminals_behind.push((self.terminal_number, front));
        }
        self.terminals_made += 1;
        self.terminal_number = self.terminals_made;
        self.terminal = Some(Box::new(terminal));
        self.terminal_hidden = false;
    }

    /// Brings a terminal, by its number, to the front.
    fn switch_terminal(&mut self, number: u32) {
        if let Some(at) = self.terminals_behind.iter().position(|(n, _)| *n == number)
            && let Some(front) = self.terminal.take()
        {
            let (n, chosen) = self.terminals_behind.remove(at);
            self.terminals_behind.push((self.terminal_number, front));
            self.terminal = Some(chosen);
            self.terminal_number = n;
        }
    }

    /// Ends the terminal in front; another, if there is one, comes forward.
    fn close_terminal(&mut self) {
        self.terminal = None;
        if let Some(at) = (0..self.terminals_behind.len()).max_by_key(|&i| self.terminals_behind[i].0) {
            let (n, next) = self.terminals_behind.remove(at);
            self.terminal = Some(next);
            self.terminal_number = n;
        } else {
            self.pending_focus = true;
        }
    }

    pub(super) fn terminal_panel(&mut self, ui: &mut Ui) {
        let mut close = false;
        let mut restart = false;
        let mut new_with: Option<crate::terminal::Shell> = None;
        let mut default_shell: Option<String> = None;
        let font_size = self.text_view_options().font_size;
        let Some(terminal) = self.terminal.as_mut() else {
            return;
        };
        if terminal.changed.swap(false, std::sync::atomic::Ordering::Relaxed) {
            ui.ctx().request_repaint();
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        let folder = terminal.folder.display().to_string();
        // Every terminal's tab, in the order they were started: its number and its folder's name.
        let name_of = |t: &crate::terminal::Terminal| t.folder.file_name().map_or_else(|| t.folder.display().to_string(), |n| n.to_string_lossy().into_owned());
        let mut tabs: Vec<(u32, String, String)> = self.terminals_behind.iter().map(|(n, t)| (*n, name_of(t), t.folder.display().to_string())).collect();
        tabs.push((self.terminal_number, name_of(terminal), folder.clone()));
        tabs.sort_by_key(|(n, ..)| *n);
        let front = self.terminal_number;
        let (mut switch, mut new) = (None, false);
        Panel::bottom("terminal-panel").default_size(240.0).resizable(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (number, name, place) in &tabs {
                    if ui.selectable_label(*number == front, RichText::new(crate::i18n::tf!("{number}: {name}")).size(12.5)).on_hover_text(place.as_str()).clicked() && *number != front {
                        switch = Some(*number);
                    }
                }
                let shells = crate::terminal::shells();
                if shells.len() < 2 {
                    if ui.small_button("+").on_hover_text(crate::i18n::tr("Another terminal")).clicked() {
                        new = true;
                    }
                } else {
                    // On Windows, which shell: PowerShell, Command Prompt, Git Bash or a WSL distribution.
                    crate::fit::menu_button(ui, "+", |ui| {
                        for shell in &shells {
                            if ui.button(shell.label()).clicked() {
                                new_with = Some(shell.clone());
                                ui.close();
                            }
                        }
                        ui.separator();
                        crate::fit::menu_button(ui, crate::i18n::tr("Open new terminals with"), |ui| {
                            for shell in &shells {
                                if ui.radio(shell.id() == self.terminal_shell, shell.label()).clicked() {
                                    default_shell = Some(shell.id());
                                    ui.close();
                                }
                            }
                        });
                    })
                    .response
                    .on_hover_text(crate::i18n::tr("Another terminal"));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Close").on_hover_text(crate::i18n::tr("Close it, ending the shell")).clicked();
                    restart = ui.small_button(crate::i18n::tr("Restart")).on_hover_text(crate::i18n::tr("A fresh shell")).clicked();
                });
            });
            crate::terminal::draw(ui, terminal, font_size);
        });
        if close {
            self.close_terminal();
        } else if restart {
            let (folder, shell) = self.terminal.as_ref().map_or((None, Default::default()), |t| (Some(t.folder.clone()), t.shell.clone()));
            self.terminal = None;
            self.new_terminal_with(folder, &shell);
            // A restarted one keeps its number and place.
            self.terminals_made -= 1;
            self.terminal_number = front;
        } else if let Some(number) = switch {
            self.switch_terminal(number);
        } else if new {
            self.new_terminal(None);
        } else if let Some(shell) = new_with {
            self.new_terminal_with(None, &shell);
        }
        if let Some(id) = default_shell {
            self.terminal_shell = id;
            self.save_settings_if_changed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    #[test]
    fn terminals_have_tabs_and_hiding_the_panel_keeps_them_running() {
        let (mut app, ctx) = app_with("");
        app.toggle_terminal();
        assert!(app.terminal.is_some() && !app.terminal_hidden);
        app.new_terminal(Some(std::env::temp_dir()));
        app.new_terminal(None);
        assert_eq!((app.terminal_number, app.terminals_behind.len()), (3, 2));
        // Drawn with its three tabs.
        let _ = ctx.run_ui(Default::default(), |ui| app.terminal_panel(ui));
        app.switch_terminal(2);
        assert_eq!(app.terminal_number, 2);
        assert_eq!(app.terminal.as_ref().unwrap().folder, std::env::temp_dir());
        // Put away and back: still the same three.
        app.toggle_terminal();
        assert!(app.terminal_hidden && app.terminal.is_some());
        app.toggle_terminal();
        assert!(!app.terminal_hidden);
        // Closing the one in front brings the newest of the rest forward.
        app.close_terminal();
        assert_eq!((app.terminal_number, app.terminals_behind.len()), (3, 1));
        app.close_terminal();
        app.close_terminal();
        assert!(app.terminal.is_none());
    }

    #[test]
    fn the_screen_designer_draws() {
        let (mut app, ctx) = app_with("");
        app.screen_designer = Some(Box::default());
        for _ in 0..3 {
            ctx.run_ui(Default::default(), |ui| app.screen_designer_window(ui.ctx())).textures_delta.clear();
        }
        assert!(app.screen_designer.is_some());
    }

    #[test]
    fn the_window_draws_in_every_kind() {
        let (mut app, ctx) = app_with("");
        for kind in Kind::ALL {
            app.pixel_editor = Some(Box::new(Editor::new(kind)));
            for _ in 0..3 {
                ctx.run_ui(Default::default(), |ui| app.pixel_editor_window(ui.ctx())).textures_delta.clear();
            }
            assert!(app.pixel_editor.is_some());
        }
    }

    #[test]
    fn characters_go_into_the_source_and_come_back_from_it() {
        let (mut app, _ctx) = app_with("");
        app.new_document();
        let mut editor = Editor::new(Kind::Characters);
        editor.sheet = Sheet::from_bytes(Kind::Characters, &[0x18, 0x3C, 0x7E, 0xFF, 0xFF, 0x7E, 0x3C, 0x18]);
        editor.label = "ship".into();
        app.pixel_editor = Some(Box::new(editor));
        // Put in the source, as the button does.
        let text = app.pixel_editor.as_ref().unwrap().sheet.to_db("ship");
        app.doc().text.type_text(&text);
        assert_eq!(&*app.doc().text.line(1), "        db $18,$3C,$7E,$FF,$FF,$7E,$3C,$18");
        // Read back from a selection.
        app.doc().text.move_caret(0, 0, false);
        app.doc().text.move_caret(2, 0, true);
        let back = Sheet::from_text(Kind::Characters, &app.doc().text.selection_text()).unwrap();
        assert_eq!(back.to_bytes(), [0x18, 0x3C, 0x7E, 0xFF, 0xFF, 0x7E, 0x3C, 0x18]);
    }

    #[test]
    fn c64_files_load_by_their_kind() {
        // A charset .prg: its load address kept, the rest characters.
        let mut prg = vec![0x00, 0x30];
        prg.extend((0..2048).map(|i| i as u8));
        let mut editor = Editor::new(Kind::Characters);
        load_into(&mut editor, Path::new("font.prg"), &prg).unwrap();
        assert_eq!((editor.sheet.kind, editor.sheet.items.len(), editor.load_address), (Kind::C64Characters, 256, 0x3000));
        assert_eq!(editor.prg(), prg);
        // SpritePad's file becomes C64 sprites, whatever was being drawn.
        let mut sprites = Editor::new(Kind::C64MultiSprites);
        sprites.sheet.items[0][5] = 3;
        let spd = sprites.sheet.to_spd();
        load_into(&mut editor, Path::new("s.spd"), &spd).unwrap();
        assert_eq!((editor.sheet.kind, editor.sheet.items.len()), (Kind::C64MultiSprites, 8));
        assert_eq!(editor.sheet.to_spd(), spd);
        assert!(load_into(&mut editor, Path::new("bad.spd"), b"nothing").is_err());
        // .spr is the Next's, unless C64 sprites are being drawn.
        let mut next = Editor::new(Kind::Characters);
        load_into(&mut next, Path::new("a.spr"), &[0; 256]).unwrap();
        assert_eq!(next.sheet.kind, Kind::Sprites);
    }
}
