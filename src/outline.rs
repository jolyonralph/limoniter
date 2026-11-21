//! The file in front's outline: its functions and labels, as its language's grammar finds them, worked out in the
//! background a moment after typing stops. Breadcrumbs above the text show where the file is and which of them the
//! cursor is in, each a menu to go somewhere else; and sticky scroll keeps the one scrolled past at the top.

use super::*;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::time::Instant;

/// How long typing has to stop before the outline is worked out again.
const PAUSE: Duration = Duration::from_millis(400);

/// The most files and folders a breadcrumb's menu lists.
const MOST_LISTED: usize = 400;

type Symbols = Vec<(usize, String)>;

#[derive(Default)]
pub(super) struct Outline {
    /// The document and version the symbols are for.
    key: Option<(u64, u64)>,
    pub symbols: Symbols,
    job: Option<((u64, u64), Arc<AtomicBool>, Receiver<Symbols>)>,
    /// When the document in front last changed, to wait for a pause.
    changed: Option<((u64, u64), Instant)>,
}

/// Where a file is, as the breadcrumbs show it: from the open folder (or project) if it's in it, else its own folder.
fn crumbs(path: &Path, root: Option<&Path>) -> Vec<PathBuf> {
    let base = root.filter(|r| path.starts_with(r)).map(Path::to_path_buf).or_else(|| path.parent().map(Path::to_path_buf)).unwrap_or_default();
    let mut out = vec![base.clone()];
    let mut here = base;
    if let Ok(rest) = path.strip_prefix(&out[0]) {
        for part in rest.components() {
            here.push(part);
            out.push(here.clone());
        }
    }
    out
}

/// A folder's menu: its folders (each a menu of its own) and files, to open one.
fn folder_menu(ui: &mut Ui, folder: &Path, open: &mut Option<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        ui.label(crate::i18n::tr("This folder can't be read."));
        return;
    };
    let mut entries: Vec<(bool, String, PathBuf)> = entries.flatten().filter(|e| !crate::workspace::hidden(&e.file_name().to_string_lossy())).map(|e| (e.path().is_dir(), e.file_name().to_string_lossy().into_owned(), e.path())).collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    if entries.is_empty() {
        ui.label(RichText::new(crate::i18n::tr("Nothing in it")).color(theme::text_faint()));
    }
    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        for (is_folder, name, path) in entries.into_iter().take(MOST_LISTED) {
            if is_folder {
                crate::fit::menu_button(ui, format!("\u{1F4C1} {name}"), |ui| folder_menu(ui, &path, open));
            } else if ui.button(name).clicked() {
                *open = Some(path);
                ui.close();
            }
        }
    });
}

impl Limoniter {
    /// Keeps the outline of the file in front up to date, in the background.
    pub(super) fn outline_tick(&mut self) {
        if !self.view.breadcrumbs && !self.view.sticky_scroll {
            return;
        }
        if let Some((key, _, receive)) = &self.outline.job
            && let Ok(found) = receive.try_recv()
        {
            self.outline.key = Some(*key);
            self.outline.symbols = found;
            self.outline.job = None;
        }
        if self.no_file() {
            return;
        }
        let doc = &mut self.docs[self.active];
        if doc.hex_mode || doc.text.is_disk_backed() || doc.placeholder {
            return;
        }
        let key = (doc.id, doc.text.version());
        if self.outline.key == Some(key) || self.outline.job.as_ref().is_some_and(|(k, ..)| *k == key) {
            return;
        }
        // Another file straight away; the same one, once typing stops.
        let now = Instant::now();
        let since = match self.outline.changed {
            Some((k, when)) if k == key => when,
            _ => {
                self.outline.changed = Some((key, now));
                now
            }
        };
        let same_file = self.outline.key.is_some_and(|(id, _)| id == key.0);
        if same_file && now.duration_since(since) < PAUSE {
            return;
        }
        if !same_file {
            self.outline.symbols.clear();
        }
        // Not before the grammars are loaded: without them, no file has a language yet.
        let Some(engine) = crate::highlight::engine() else { return };
        let Some(syntax) = doc.syntax_index() else {
            self.outline.key = Some(key);
            self.outline.symbols.clear();
            return;
        };
        if let Some((_, stop, _)) = self.outline.job.take() {
            stop.store(true, Ordering::Relaxed);
        }
        let lines = doc.text.all_lines();
        let stop = Arc::new(AtomicBool::new(false));
        let (send, receive) = std::sync::mpsc::channel();
        let worker_stop = stop.clone();
        std::thread::spawn(move || {
            let found = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| engine.list_functions(syntax, &lines, &worker_stop))).unwrap_or_default();
            let _ = send.send(super::workspace_host::tidy(found));
        });
        self.outline.job = Some((key, stop, receive));
    }

    /// The outline's symbols, if they're for the document at this index.
    fn outline_of(&self, index: usize) -> Option<&Symbols> {
        let id = self.docs.get(index)?.id;
        self.outline.key.is_some_and(|(k, _)| k == id).then_some(&self.outline.symbols)
    }

    /// Whether the outline is the front document's.
    pub(super) fn outline_of_front(&self) -> bool {
        self.outline_of(self.active).is_some()
    }

    /// The line sticky scroll keeps at the top of a document: the last function or label above the first line shown.
    pub(super) fn sticky_line(&self, index: usize) -> Option<usize> {
        if !self.view.sticky_scroll {
            return None;
        }
        let top = self.docs.get(index)?.text_state.top;
        self.outline_of(index)?.iter().rev().find(|(line, _)| *line < top).map(|(line, _)| *line)
    }

    /// The bar above the text: the file's folders and its name, then the function or label the cursor is in. Each
    /// is a menu: a folder's files to open, or the file's functions and labels to go to.
    pub(super) fn breadcrumbs(&mut self, ui: &mut Ui, index: usize) {
        let Some(path) = self.docs[index].path.clone() else { return };
        let root = self.project.as_ref().and_then(|p| p.root.clone());
        let places = crumbs(&path, root.as_deref());
        let caret = self.docs[index].text.caret_line;
        let symbols = self.outline_of(index).cloned().unwrap_or_default();
        let here = symbols.iter().rev().find(|(line, _)| *line <= caret).cloned();
        let (mut open, mut go) = (None, None);
        egui::Frame::new().fill(theme::editor_back()).inner_margin(egui::Margin { left: 8, right: 8, top: 1, bottom: 1 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.style_mut().visuals.widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
                let small = |text: String| RichText::new(text).size(12.0).color(theme::text_dim());
                for (n, place) in places.iter().enumerate() {
                    if n > 0 {
                        ui.label(small("\u{203A}".into()));
                    }
                    let name = place.file_name().map_or_else(|| place.display().to_string(), |n| n.to_string_lossy().into_owned());
                    // The file's own crumb lists what's beside it; a folder's, what's in it.
                    let folder = if *place == path { place.parent().map(Path::to_path_buf).unwrap_or_default() } else { place.clone() };
                    crate::fit::menu_button(ui, small(name), |ui| folder_menu(ui, &folder, &mut open)).response.on_hover_text(place.display().to_string());
                }
                if let Some((_, name)) = &here {
                    ui.label(small("\u{203A}".into()));
                    crate::fit::menu_button(ui, small(name.clone()), |ui| {
                        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                            for (line, symbol) in &symbols {
                                if ui.button(RichText::new(symbol).monospace()).on_hover_text(crate::i18n::tf!("Line {}", line + 1)).clicked() {
                                    go = Some(*line);
                                    ui.close();
                                }
                            }
                        });
                    });
                }
            });
        });
        if let Some(line) = go {
            let doc = &mut self.docs[index];
            doc.text.move_caret(line, 0, false);
            doc.text.reveal = true;
            self.pending_focus = true;
        }
        if let Some(path) = open {
            self.open_path(&path);
            self.pending_focus = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    #[test]
    fn crumbs_run_from_the_folder_open_to_the_file() {
        let root = Path::new("/work/game");
        assert_eq!(crumbs(Path::new("/work/game/src/gfx/sprites.s"), Some(root)), [PathBuf::from("/work/game"), PathBuf::from("/work/game/src"), PathBuf::from("/work/game/src/gfx"), PathBuf::from("/work/game/src/gfx/sprites.s")]);
        // Outside it, from the file's own folder.
        assert_eq!(crumbs(Path::new("/elsewhere/a.s"), Some(root)), [PathBuf::from("/elsewhere"), PathBuf::from("/elsewhere/a.s")]);
    }

    /// The outline comes in the background; sticky scroll then keeps the label above the first line shown.
    #[test]
    fn the_outline_follows_the_file_and_sticky_scroll_uses_it() {
        crate::highlight::load_now();
        let (mut app, _ctx) = app_with("");
        let file = std::env::temp_dir().join(format!("limoniter-outline-{}.s", std::process::id()));
        let mut text = String::from("start:\tmove.l\td0,d1\n\tlea\tdata,a0\n\tmoveq\t#1,d2\n");
        text += &"\tnop\n".repeat(40);
        text += "loop:\tmove.w\td0,(a0)+\n\tdbra\td1,loop\n\trts\n";
        std::fs::write(&file, &text).unwrap();
        assert!(app.open_path(&file));
        for _ in 0..200 {
            app.outline_tick();
            if app.outline_of(app.active).is_some_and(|s| !s.is_empty()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let names: Vec<String> = app.outline_of(app.active).unwrap().iter().map(|(_, n)| n.clone()).collect();
        assert_eq!(names, ["start", "loop"]);
        app.doc().text_state.top = 20;
        assert_eq!(app.sticky_line(app.active), Some(0));
        app.doc().text_state.top = 0;
        assert_eq!(app.sticky_line(app.active), None, "nothing scrolled past");
        app.view.sticky_scroll = false;
        app.doc().text_state.top = 20;
        assert_eq!(app.sticky_line(app.active), None);
        let _ = std::fs::remove_file(file);
    }
}
