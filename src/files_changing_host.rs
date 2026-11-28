//! The window's side of the Files changing panel (View, Panels): what's watched, drawn in a side panel, and what a
//! click on a row does. While the panel is shut nothing is watched.

use super::*;

impl Limoniter {
    /// The folders the panel watches now: the project's folder where one is open, else the folders of open files.
    fn files_changing_source(&self) -> crate::files_changing::Source {
        let open: Vec<PathBuf> = self.docs.iter().filter_map(|d| d.path.clone()).collect();
        match &self.project {
            Some(project) => {
                let on_server = project.remote.is_some();
                let hidden = project.explorer.hidden.as_slice();
                // A project that's a list of files watches the folders they're in.
                let mut places = open;
                if project.root.is_none() {
                    places.extend(project.files.iter().cloned());
                }
                crate::files_changing::source_for(self.agents.wt.view.as_deref().or(project.root.as_deref()), hidden, on_server, &places)
            }
            None => crate::files_changing::source_for(None, &[], false, &open),
        }
    }

    pub(super) fn files_changing_panel(&mut self, ui: &mut Ui) {
        if !self.panels.files_changing {
            self.files_changing.stop();
            return;
        }
        let source = self.files_changing_source();
        self.files_changing.update(&source, ui.ctx());
        let open: Vec<PathBuf> = self.docs.iter().filter_map(|d| d.path.clone()).collect();
        let mut shown = true;
        let mut acts = Vec::new();
        let panel = &mut self.files_changing;
        panels::dock(ui, false, "files-changing-panel", "Files changing", 260.0, &mut shown, |ui| acts = panel.draw(ui, source.on_server, &|p| open.iter().any(|o| same_path(o, p))));
        self.panels.files_changing = shown;
        for act in acts {
            match act {
                crate::files_changing::Act::Open(path) => {
                    self.open_path(&path);
                }
                crate::files_changing::Act::Reveal(path) => {
                    if let Err(problem) = crate::platform::reveal(&path) {
                        self.message(crate::platform::SHOW_IN_FILE_MANAGER, problem);
                    }
                }
            }
        }
    }

    /// A step before a screenshot: the panel shown, with a few files made, changed and deleted in the project's
    /// folder (or the folder of the file in front), and an agent's claim on one of them.
    pub(super) fn files_changing_screenshot_step(&mut self, _value: &str) {
        self.panels.files_changing = true;
        let folder = self.project.as_ref().map(|p| p.folder()).or_else(|| self.docs.get(self.active).and_then(|d| d.path.as_deref()).and_then(Path::parent).map(Path::to_path_buf));
        let Some(folder) = folder else { return };
        let ctx = self.waker.clone().unwrap_or_default();
        let source = self.files_changing_source();
        self.files_changing.update(&source, &ctx);
        std::thread::sleep(Duration::from_millis(500));
        let _ = std::fs::create_dir_all(folder.join("src"));
        let _ = std::fs::create_dir_all(folder.join("docs"));
        let own = folder.join("main.php");
        let _ = std::fs::write(&own, "<?php echo 1;\n");
        crate::files_changing::note_own_write(&own);
        crate::files_changing::claim(&folder.join("src"), "Claude Code");
        let _ = std::fs::write(folder.join("src").join("index.php"), "<?php\n");
        let _ = std::fs::write(folder.join("docs").join("index.php"), "<?php\n");
        let _ = std::fs::write(folder.join("notes.txt"), "a\n");
        let _ = std::fs::write(folder.join("gone.txt"), "a\n");
        std::thread::sleep(Duration::from_millis(800));
        let _ = std::fs::remove_file(folder.join("gone.txt"));
        let began = std::time::Instant::now();
        while self.files_changing.rows.len() < 6 && began.elapsed() < Duration::from_secs(8) {
            std::thread::sleep(Duration::from_millis(100));
            self.files_changing.update(&source, &ctx);
        }
        std::thread::sleep(Duration::from_millis(1000));
        self.files_changing.update(&source, &ctx);
    }
}
