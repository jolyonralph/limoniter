//! The window's side of Workbench icons (Amiga, Workbench icon): the icon in front shown as each of its pictures,
//! with its kind, default tool, stack, place and tool types to change. Changes go into the document as one
//! undoable change, to be saved as any file is; an icon whose details aren't changed is left byte for byte as it
//! was. And, when Run on, Amiga crunchers and music says so, an icon made for each Amiga program F9 builds.

use super::*;
use crate::amiga_icon::{self as icon, Details, Icon};

pub(super) struct IconBox {
    doc_id: u64,
    icon: Icon,
    /// The details as the icon has them, and as they're being changed.
    saved: Details,
    details: Details,
    tool_types: String,
    pictures: Vec<(String, egui::TextureHandle, Option<egui::TextureHandle>, egui::Vec2)>,
    read_only: bool,
    status: String,
}

impl IconBox {
    /// The details as the boxes have them now.
    fn edited(&self) -> Details {
        let mut details = self.details.clone();
        details.tool_types = self.tool_types.lines().map(|l| l.trim_end_matches('\r').to_string()).filter(|l| !l.is_empty()).collect();
        details
    }
}

impl Limoniter {
    /// Opens the icon in front in the icon window, or says why it can't.
    pub(super) fn open_amiga_icon(&mut self, ctx: &Context) {
        if self.no_file() {
            return;
        }
        let bytes = self.document_bytes();
        let doc = &self.docs[self.active];
        let icon = match Icon::read(&bytes) {
            Ok(icon) => icon,
            Err(problem) => return self.message("Workbench icon", crate::i18n::tf!("{} isn't a Workbench icon Limoniter can read. {problem}", doc.name())),
        };
        let options = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, minification: egui::TextureFilter::Linear, ..egui::TextureOptions::default() };
        let pictures = icon
            .pictures()
            .into_iter()
            .enumerate()
            .map(|(n, shown)| {
                let normal = ctx.load_texture(format!("icon-{n}"), egui::ColorImage::from_rgba_unmultiplied([shown.width, shown.height], &shown.normal), options);
                let selected = shown.selected.as_ref().map(|rgba| ctx.load_texture(format!("icon-{n}-selected"), egui::ColorImage::from_rgba_unmultiplied([shown.width, shown.height], rgba), options));
                let colours = if shown.colours > 0 { crate::i18n::tf!("{} \u{D7} {}, {} colours", shown.width, shown.height, shown.colours) } else { crate::i18n::tf!("{} \u{D7} {}, true colour", shown.width, shown.height) };
                (format!("{}: {colours}", crate::i18n::tr(shown.kind.name())), normal, selected, egui::vec2(shown.width as f32 * shown.aspect, shown.height as f32))
            })
            .collect();
        let details = icon.details();
        let read_only = !doc.hex_mode || doc.hex.read_only || doc.browser.is_some();
        let tool_types = details.tool_types.join("\n");
        self.dialog = Dialog::AmigaIcon(Box::new(IconBox { doc_id: doc.id, icon, saved: details.clone(), details, tool_types, pictures, read_only, status: String::new() }));
    }

    pub(super) fn amiga_icon_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::AmigaIcon(state) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut apply = false;
        let changed = state.edited() != state.saved;
        crate::fit::Dialog::new(crate::i18n::tr("Workbench icon")).collapsible(false).resizable(true).default_width(560.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show_with_buttons(ctx, |ui, part| match part {
            crate::fit::Part::Body => {
                ui.set_max_width(640.0);
                egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
                    ui.label(RichText::new(crate::i18n::tr("As Workbench shows it, and selected (double-clicked):")).color(theme::text_dim()).size(12.5));
                    for (label, normal, selected, size) in &state.pictures {
                        ui.add_space(4.0);
                        ui.label(RichText::new(label).size(12.5));
                        // As large as fits in whole steps, up to four times.
                        let scale = (200.0 / size.x.max(size.y)).clamp(1.0, 4.0).floor();
                        ui.horizontal(|ui| {
                            ui.add(egui::Image::new(normal).maintain_aspect_ratio(false).fit_to_exact_size(*size * scale));
                            if let Some(selected) = selected {
                                ui.add_space(12.0);
                                ui.add(egui::Image::new(selected).maintain_aspect_ratio(false).fit_to_exact_size(*size * scale));
                            }
                        });
                    }
                    ui.separator();
                    ui.add_enabled_ui(!state.read_only, |ui| {
                        egui::Grid::new("icon-details").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                            ui.label(crate::i18n::tr("Kind:"));
                            egui::ComboBox::from_id_salt("icon-kind").selected_text(crate::i18n::tr(icon::type_name(state.details.kind))).show_ui(ui, |ui| {
                                for kind in icon::TYPES {
                                    ui.selectable_value(&mut state.details.kind, kind, crate::i18n::tr(icon::type_name(kind)));
                                }
                            });
                            ui.end_row();
                            ui.label(crate::i18n::tr("Default tool:"));
                            ui.add(egui::TextEdit::singleline(&mut state.details.default_tool).hint_text(crate::i18n::tr("The program a project opens with, such as SYS:Utilities/MultiView")).desired_width(360.0));
                            ui.end_row();
                            ui.label(crate::i18n::tr("Stack:"));
                            ui.horizontal(|ui| {
                                ui.add(egui::DragValue::new(&mut state.details.stack).range(0..=16 * 1024 * 1024).speed(64.0));
                                ui.label(RichText::new(crate::i18n::tr("bytes, for the program started from it (4096 if none)")).color(theme::text_dim()).size(12.0));
                            });
                            ui.end_row();
                            ui.label(crate::i18n::tr("Place:"));
                            ui.horizontal(|ui| match state.details.position {
                                Some((x, y)) => {
                                    ui.label(crate::i18n::tf!("{x}, {y} in its window"));
                                    if ui.small_button(crate::i18n::tr("Let Workbench place it")).clicked() {
                                        state.details.position = None;
                                    }
                                }
                                None => {
                                    ui.label(crate::i18n::tr("Placed by Workbench"));
                                    if let Some(at) = state.saved.position
                                        && ui.small_button(crate::i18n::tr("Put back")).clicked()
                                    {
                                        state.details.position = Some(at);
                                    }
                                }
                            });
                            ui.end_row();
                            ui.label(crate::i18n::tr("Tool types:"));
                            ui.add(egui::TextEdit::multiline(&mut state.tool_types).hint_text(crate::i18n::tr("One to a line, such as DONOTWAIT or PUBSCREEN=Workbench")).desired_rows(6).desired_width(360.0).font(egui::TextStyle::Monospace));
                            ui.end_row();
                        });
                    });
                });
            }
            crate::fit::Part::Buttons => {
                ui.separator();
                if state.read_only {
                    ui.label(RichText::new(crate::i18n::tr("This file can't be changed here: open it from a folder, in hex, to change its icon.")).color(theme::text_dim()));
                }
                if !state.status.is_empty() {
                    ui.colored_label(theme::change_saved(), &state.status);
                }
                ui.horizontal(|ui| {
                    apply = ui.add_enabled(changed && !state.read_only, Button::new(crate::i18n::tr("Change the icon"))).on_hover_text(crate::i18n::tr("Puts the changes into the file, to be saved as any change is; nothing else in it changes")).clicked();
                    if ui.add_enabled(changed, Button::new(crate::i18n::tr("Undo the changes"))).clicked() {
                        state.details = state.saved.clone();
                        state.tool_types = state.saved.tool_types.join("\n");
                    }
                    close |= ui.button(crate::i18n::tr("Close")).clicked();
                });
            }
        });
        if apply {
            let doc_id = state.doc_id;
            let edited = state.edited();
            let mut changed_icon = state.icon.clone();
            changed_icon.set_details(&edited);
            let bytes = changed_icon.to_bytes();
            if let Some(doc) = self.docs.iter_mut().find(|d| d.id == doc_id && d.hex_mode) {
                doc.hex.replace_everything(bytes);
                if let Dialog::AmigaIcon(state) = &mut self.dialog {
                    state.icon = changed_icon;
                    state.saved = edited;
                    state.status = crate::i18n::tr("Changed in the file: save it (Ctrl+S) to keep the changes.").into();
                }
                self.preview_of = None;
            }
        }
        if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }

    /// After F9 builds an Amiga program, makes a Workbench icon for it beside it when the setting asks: from the
    /// person's own icon, or Limoniter's. `saved` says whether the program is already beside the source (a
    /// compiled one); an assembled one is saved there for its icon to start.
    pub(super) fn icon_after_build(&mut self, doc_id: u64, name: &str, bytes: &[u8], saved: bool) {
        if !crate::amiga_tools::is_hunk_executable(bytes) {
            return;
        }
        let prefs = crate::amiga_tools::Prefs::load();
        if !prefs.icon_after_build {
            return;
        }
        let Some(folder) = self.docs.iter().find(|d| d.id == doc_id).and_then(|d| d.path.as_deref()).and_then(Path::parent).map(Path::to_path_buf) else {
            return;
        };
        let theirs = prefs.default_icon.as_deref().map(|path| std::fs::read(path).map_err(|e| crate::i18n::tf!("Your icon, {}, couldn't be read: {e}", path.display())));
        let made = match theirs {
            Some(Ok(bytes)) => icon::program_icon(Some(&bytes)).map_err(|problem| crate::i18n::tf!("Your icon couldn't be used, so Limoniter's own was: {problem}")),
            Some(Err(problem)) => Err(problem),
            None => Ok(icon::own_program_icon()),
        };
        let made = made.unwrap_or_else(|problem| {
            self.script_print(problem, false);
            icon::own_program_icon()
        });
        if !saved {
            let program = folder.join(name);
            if let Err(e) = crate::codec::write_file(&program, bytes) {
                self.script_print(crate::i18n::tf!("{} couldn't be saved for its icon: {e}", program.display()), false);
                return;
            }
        }
        let path = folder.join(icon::icon_name(name));
        match crate::codec::write_file(&path, &made) {
            Ok(()) => self.script_print(crate::i18n::tf!("Made the Workbench icon {}.", path.display()), false),
            Err(e) => self.script_print(crate::i18n::tf!("{} couldn't be saved: {e}", path.display()), false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{app_with, draw};

    fn file(name: &str, bytes: &[u8]) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("limoniter-icon-host-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn the_box(app: &mut Limoniter) -> &mut IconBox {
        match &mut app.dialog {
            Dialog::AmigaIcon(state) => state,
            _ => panic!("the icon window isn't open"),
        }
    }

    #[test]
    fn an_icon_opens_as_a_picture_and_its_tool_types_are_changed_and_saved() {
        let (mut app, ctx) = app_with("");
        let bytes = crate::amiga_icon::tests::made(&["DONOTWAIT"], Some("SYS:Utilities/MultiView"), false);
        let path = file("Readme.info", &bytes);
        assert!(app.open_path(&path));
        assert!(app.docs[app.active].hex_mode, "an icon opens in hex");
        assert_eq!(app.docs[app.active].picture, Some(crate::pictures::Format::AmigaIcon));
        draw(&mut app, &ctx, Vec::new());
        let (picture, _) = app.preview_picture.as_ref().expect("shown as a picture");
        assert!(picture.info.iter().any(|(l, v)| l == "Tool types" && v == "DONOTWAIT"), "{:?}", picture.info);
        app.open_amiga_icon(&ctx);
        draw(&mut app, &ctx, Vec::new());
        let state = the_box(&mut app);
        assert!(!state.read_only);
        assert_eq!(state.pictures.len(), 1);
        assert_eq!(state.tool_types, "DONOTWAIT");
        // Unchanged, nothing can be put in, and the file is as it was.
        assert_eq!(state.edited(), state.saved);
        state.tool_types = "DONOTWAIT\nPUBSCREEN=Workbench\n\n".into();
        state.details.stack = 8192;
        let edited = state.edited();
        assert_eq!(edited.tool_types, ["DONOTWAIT", "PUBSCREEN=Workbench"], "blank lines aren't tool types");
        // The window's button, as clicking it does.
        let mut changed = state.icon.clone();
        changed.set_details(&edited);
        let doc_id = state.doc_id;
        app.docs.iter_mut().find(|d| d.id == doc_id).unwrap().hex.replace_everything(changed.to_bytes());
        assert!(app.docs[app.active].is_modified());
        assert!(app.save(app.active));
        let written = std::fs::read(&path).unwrap();
        let read = Icon::read(&written).unwrap().details();
        assert_eq!(read.tool_types, ["DONOTWAIT", "PUBSCREEN=Workbench"]);
        assert_eq!((read.stack, read.default_tool.as_str()), (8192, "SYS:Utilities/MultiView"));
        // Undo puts the icon back as it was, byte for byte.
        app.docs[app.active].hex.undo();
        assert_eq!(app.docs[app.active].hex.bytes(), bytes);
    }

    #[test]
    fn something_that_isnt_an_icon_says_so() {
        let (mut app, ctx) = app_with("");
        let path = file("notes.txt", b"just some words");
        assert!(app.open_path(&path));
        app.open_amiga_icon(&ctx);
        assert!(!matches!(app.dialog, Dialog::AmigaIcon(_)));
    }

    #[test]
    fn an_icon_is_made_beside_a_program_built_when_the_setting_says() {
        let (mut app, _ctx) = app_with("");
        let source = file("demo.s", b"\trts\n");
        assert!(app.open_path(&source));
        let id = app.docs[app.active].id;
        let program: Vec<u8> = [0x3F3u32, 0, 1, 0, 0, 1, 0x3E9, 1, 0x4E75_0000, 0x3F2].iter().flat_map(|w| w.to_be_bytes()).collect();
        let folder = source.parent().unwrap().to_path_buf();
        // Off, as it starts: nothing is made.
        app.icon_after_build(id, "demo.exe", &program, false);
        assert!(!folder.join("demo.exe.info").exists());
        let mut prefs = crate::amiga_tools::Prefs::load();
        prefs.icon_after_build = true;
        prefs.save();
        app.icon_after_build(id, "demo.exe", &program, false);
        assert_eq!(std::fs::read(folder.join("demo.exe")).unwrap(), program, "the assembled program is saved for its icon");
        assert_eq!(std::fs::read(folder.join("demo.exe.info")).unwrap(), icon::own_program_icon());
        // From the person's own icon, which keeps its tool types.
        let theirs = file("mine.info", &crate::amiga_icon::tests::made(&["TOOLPRI=-1"], Some("C:Ed"), true));
        prefs.default_icon = Some(theirs);
        prefs.save();
        app.icon_after_build(id, "tool", &program, true);
        let made = Icon::read(&std::fs::read(folder.join("tool.info")).unwrap()).unwrap().details();
        assert_eq!((made.kind, made.default_tool.as_str(), made.tool_types.clone()), (icon::TOOL, "", vec!["TOOLPRI=-1".to_string()]));
        assert!(!folder.join("tool").exists(), "a compiled program, saved already, isn't saved again");
        // Something that isn't an Amiga program gets no icon.
        app.icon_after_build(id, "data", b"not a program at all, but long enough", true);
        assert!(!folder.join("data.info").exists());
        prefs.icon_after_build = false;
        prefs.default_icon = None;
        prefs.save();
    }
}
