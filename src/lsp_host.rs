//! The window's side of language servers. With them switched on (Settings, they start off), the file in front is
//! looked after by the server for its language, started in its project's folder: told of the file and its changes
//! a moment after typing stops, asked for words to finish one being typed and about what's under the pointer, and
//! asked by F12, Shift+F12 and Ctrl+F12 for definitions, uses and renames. Its problems are marked by their lines.

use super::*;
use crate::lsp::{self, Server, ServerSetting};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

/// What's been asked of a server and not yet answered.
enum Pending {
    /// Words for a place: the document, the line, and where the word starts.
    Completion(u64, usize, usize),
    /// About a place: the document, line and column.
    Hover(u64, usize, usize),
}

#[derive(Default)]
pub(super) struct LspState {
    /// Whether servers are used at all; off until switched on.
    pub on: bool,
    pub settings: Vec<ServerSetting>,
    /// Servers running, by command and folder.
    servers: Vec<(String, Server)>,
    /// Each document told to a server: which, and the version last sent, and when it was last changed.
    opened: HashMap<u64, (usize, u64, PathBuf)>,
    pending: Vec<(usize, i64, Pending)>,
    /// What the pointer was last asked about: the document, line and where its word starts.
    last_hover: Option<(u64, usize, usize)>,
    /// Languages whose server couldn't be started, not tried again this session.
    failed: BTreeSet<String>,
    /// When each document last changed and to what version, to tell the server a moment after.
    changed: HashMap<u64, (u64, std::time::Instant)>,
}

impl LspState {
    pub fn from_settings(settings: &str) -> Self {
        let on = list_setting(settings, "lsp_on").first().is_some_and(|v| v == "true");
        let servers = if list_setting(settings, "lsp_servers").iter().any(|v| v == "changed") {
            list_setting(settings, "language_server").iter().filter_map(|v| v.split_once('\u{1}')).map(|(l, c)| ServerSetting { language: l.into(), command: c.into() }).collect()
        } else {
            lsp::usual_servers()
        };
        LspState { on, settings: servers, ..Default::default() }
    }

    pub fn to_settings(&self) -> String {
        let mut text = format!("lsp_on={}\n", self.on);
        if self.settings != lsp::usual_servers() {
            text.push_str("lsp_servers=changed\n");
            for s in &self.settings {
                text.push_str(&format!("language_server={}\u{1}{}\n", s.language, s.command));
            }
        }
        text
    }

    /// Stops every server.
    pub fn stop_all(&mut self) {
        for (_, server) in &mut self.servers {
            server.stop();
        }
        self.servers.clear();
        self.opened.clear();
        self.pending.clear();
    }
}

/// The text of a document as one string, as a server is given it.
fn whole_text(doc: &Document) -> String {
    doc.text.all_lines().iter().map(|l| l.as_ref()).collect::<Vec<&str>>().join("\n")
}

impl Limoniter {
    /// The server looking after the document in front, if there is one and it's ready, with the file's path.
    fn lsp_server_for(&self, index: usize) -> Option<(usize, PathBuf)> {
        let doc = self.docs.get(index)?;
        let (server, _, _) = self.lsp.opened.get(&doc.id)?;
        Some((*server, doc.path.clone()?))
    }

    /// Keeps the servers up to date with the file in front, and takes in what they say. True while waiting on one.
    pub(super) fn lsp_tick(&mut self) -> bool {
        if !self.lsp.on {
            if !self.lsp.servers.is_empty() {
                self.lsp.stop_all();
                for doc in &mut self.docs {
                    if let Some(intel) = doc.text.intel.as_mut() {
                        intel.lsp = false;
                        intel.diagnostics.clear();
                    }
                }
            }
            return false;
        }
        if !self.no_file() {
            self.lsp_follow(self.active);
        }
        // Files closed here are closed with their server.
        let gone: Vec<u64> = self.lsp.opened.keys().copied().filter(|id| !self.docs.iter().any(|d| d.id == *id)).collect();
        for id in gone {
            if let Some((server, _, path)) = self.lsp.opened.remove(&id)
                && let Some((_, server)) = self.lsp.servers.get_mut(server)
            {
                server.did_close(&path);
            }
        }
        self.lsp_take_in();
        !self.lsp.pending.is_empty() || self.lsp.servers.iter().any(|(_, s)| !s.ready())
    }

    fn lsp_follow(&mut self, index: usize) {
        let doc = &self.docs[index];
        let Some(path) = doc.path.clone().filter(|_| !doc.hex_mode && !doc.text.is_disk_backed() && doc.browser.is_none()) else {
            return;
        };
        let language = doc.language_name();
        let Some(setting) = self.lsp.settings.iter().find(|s| s.language == language).cloned() else {
            return;
        };
        if self.lsp.failed.contains(&language) {
            return;
        }
        let root = lsp::root_for(&path);
        let key = format!("{}\u{1}{}", setting.command, root.display());
        let server = match self.lsp.servers.iter().position(|(k, _)| *k == key) {
            Some(i) => i,
            None => match Server::start(&setting.command, &root) {
                Ok(server) => {
                    self.script_print(format!("Started {} for {language}, in {}", setting.command, root.display()), false);
                    self.lsp.servers.push((key, server));
                    self.lsp.servers.len() - 1
                }
                Err(problem) => {
                    self.lsp.failed.insert(language.clone());
                    self.script_print(format!("{problem}. It's in Settings, Editing, Language servers."), false);
                    return;
                }
            },
        };
        let doc_id = self.docs[index].id;
        let doc = &mut self.docs[index];
        let intel = doc.text.intel.get_or_insert_with(Box::default);
        intel.lsp = true;
        if !self.lsp.servers[server].1.ready() {
            return;
        }
        let version = doc.text.version();
        match self.lsp.opened.get(&doc_id).cloned() {
            None => {
                let text = whole_text(doc);
                self.lsp.servers[server].1.did_open(&path, &language, version, &text);
                self.lsp.opened.insert(doc_id, (server, version, path.clone()));
            }
            Some((_, sent, _)) if sent != version => {
                // A moment after the last change.
                let now = std::time::Instant::now();
                let since = match self.lsp.changed.get(&doc_id) {
                    Some((v, when)) if *v == version => *when,
                    _ => {
                        self.lsp.changed.insert(doc_id, (version, now));
                        now
                    }
                };
                if now.duration_since(since) >= Duration::from_millis(250) {
                    let text = whole_text(doc);
                    self.lsp.servers[server].1.did_change(&path, version, &text);
                    self.lsp.opened.insert(doc_id, (server, version, path.clone()));
                }
            }
            Some(_) => {}
        }
        // What the editor wants: words to finish one, and what's under the pointer.
        let doc = &mut self.docs[index];
        if let Some((line, col)) = doc.text_state.completion_wanted.take() {
            // The server needs the latest text to answer about it.
            if self.lsp.opened.get(&doc_id).is_some_and(|(_, v, _)| *v != version) {
                let text = whole_text(doc);
                self.lsp.servers[server].1.did_change(&path, version, &text);
                self.lsp.opened.insert(doc_id, (server, version, path.clone()));
            }
            let text = doc.text.line(line).to_string();
            let start = crate::code_intel::prefix_at(&text, col).map_or(col, |(s, _)| s);
            let id = self.lsp.servers[server].1.at("textDocument/completion", &path, line, lsp::to_utf16(&text, col));
            self.lsp.pending.push((server, id, Pending::Completion(doc_id, line, start)));
        }
        let doc = &self.docs[index];
        if let Some((line, col)) = doc.text_state.hover_at {
            let text = doc.text.line(line).to_string();
            if let Some((start, _, _)) = crate::code_intel::word_at(&text, col)
                && self.lsp.last_hover != Some((doc_id, line, start))
            {
                self.lsp.last_hover = Some((doc_id, line, start));
                let id = self.lsp.servers[server].1.at("textDocument/hover", &path, line, lsp::to_utf16(&text, col));
                self.lsp.pending.push((server, id, Pending::Hover(doc_id, line, col)));
            }
        }
    }

    /// What the servers have said: problems for files, and answers to what was asked.
    fn lsp_take_in(&mut self) {
        let mut said: Vec<(usize, Value)> = Vec::new();
        for (index, (_, server)) in self.lsp.servers.iter_mut().enumerate() {
            said.extend(server.poll().into_iter().map(|m| (index, m)));
        }
        for (server, message) in said {
            if message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics") {
                if let Some((path, problems)) = lsp::diagnostics(&message)
                    && let Some(doc) = self.docs.iter_mut().find(|d| d.path.as_deref().is_some_and(|p| same_path(p, &path)))
                {
                    let intel = doc.text.intel.get_or_insert_with(Box::default);
                    intel.lsp = true;
                    intel.diagnostics = problems;
                }
                continue;
            }
            let Some(id) = message.get("id").and_then(Value::as_i64) else {
                continue;
            };
            let Some(at) = self.lsp.pending.iter().position(|(s, i, _)| *s == server && *i == id) else {
                continue;
            };
            let (_, _, pending) = self.lsp.pending.remove(at);
            let result = message.get("result").cloned().unwrap_or(Value::Null);
            match pending {
                Pending::Completion(doc_id, line, start) => {
                    let words = lsp::completion_words(&result);
                    if let Some(intel) = self.docs.iter_mut().find(|d| d.id == doc_id).and_then(|d| d.text.intel.as_mut())
                        && !words.is_empty()
                    {
                        intel.lsp_completion = Some((line, start, words));
                        intel.lsp_fresh = true;
                    }
                }
                Pending::Hover(doc_id, line, col) => {
                    if let Some(intel) = self.docs.iter_mut().find(|d| d.id == doc_id).and_then(|d| d.text.intel.as_mut()) {
                        intel.lsp_hover = lsp::hover_text(&result).map(|text| (line, col, text));
                    }
                }
            }
        }
    }

    /// Asks the server about the word at the cursor, waiting for its answer a few seconds.
    fn lsp_ask_here(&mut self, method: &str, what: &str) -> Option<(Value, PathBuf)> {
        let Some((server, path)) = self.lsp_server_for(self.active) else {
            self.message(what, "This works in assembler source, and with a language server for the file's language (Settings, Editing, Language servers).".into());
            return None;
        };
        let doc = &self.docs[self.active];
        let (line, col) = (doc.text.caret_line, doc.text.caret_col);
        let text = doc.text.line(line).to_string();
        let version = doc.text.version();
        let whole = whole_text(doc);
        let doc_id = doc.id;
        let server_ref = &mut self.lsp.servers[server].1;
        if self.lsp.opened.get(&doc_id).is_some_and(|(_, v, _)| *v != version) {
            server_ref.did_change(&path, version, &whole);
            self.lsp.opened.insert(doc_id, (server, version, path.clone()));
        }
        let server_ref = &mut self.lsp.servers[server].1;
        let id = server_ref.at(method, &path, line, lsp::to_utf16(&text, col));
        match server_ref.wait_for(id, Duration::from_secs(4)) {
            Some(result) => Some((result, path)),
            None => {
                let name = self.lsp.servers[server].1.name.clone();
                self.message(what, format!("The language server ({name}) didn't answer in time. It may still be reading the project; try again in a moment."));
                None
            }
        }
    }

    /// The text of a line of a file, from its tab if it's open, else from the disk.
    fn line_of_path(&self, path: &Path, line: usize) -> String {
        if let Some(doc) = self.docs.iter().find(|d| d.path.as_deref().is_some_and(|p| same_path(p, path))) {
            return if line < doc.text.line_count() { doc.text.line(line).to_string() } else { String::new() };
        }
        std::fs::read_to_string(path).ok().and_then(|t| t.lines().nth(line).map(str::to_string)).unwrap_or_default()
    }

    pub(super) fn lsp_go_to_definition(&mut self) {
        let Some((result, _)) = self.lsp_ask_here("textDocument/definition", "Go to definition") else {
            return;
        };
        let Some(place) = lsp::locations(&result).into_iter().next() else {
            return self.message("Go to definition", "The language server doesn't know where that's defined.".into());
        };
        let col = lsp::from_utf16(&self.line_of_path(&place.path, place.line), place.units);
        self.go_to_place(&place.path.display().to_string(), place.line + 1);
        let doc = &mut self.docs[self.active];
        if !doc.hex_mode {
            doc.text.move_caret(place.line, col, false);
            doc.text.reveal = true;
        }
    }

    pub(super) fn lsp_find_uses(&mut self) {
        let Some((result, _)) = self.lsp_ask_here("textDocument/references", "Find all uses") else {
            return;
        };
        let places = lsp::locations(&result);
        let doc = &self.docs[self.active];
        let word = crate::code_intel::word_at(&doc.text.line(doc.text.caret_line), doc.text.caret_col).map(|w| w.2).unwrap_or_default();
        self.script_print(format!("{word}: {} place{}", places.len(), if places.len() == 1 { "" } else { "s" }), true);
        for place in places {
            let text = self.line_of_path(&place.path, place.line);
            self.script_print(format!("{}({}): {}", place.path.display(), place.line + 1, text.trim()), false);
        }
    }

    /// Renames what's at the cursor, as the server works out, in every file it names.
    pub(super) fn lsp_rename(&mut self, new_name: &str) {
        let Some((server, path)) = self.lsp_server_for(self.active) else {
            return;
        };
        let doc = &self.docs[self.active];
        let (line, col) = (doc.text.caret_line, doc.text.caret_col);
        let text = doc.text.line(line).to_string();
        let server_ref = &mut self.lsp.servers[server].1;
        let id = server_ref.rename(&path, line, lsp::to_utf16(&text, col), new_name);
        let Some(result) = server_ref.wait_for(id, Duration::from_secs(5)) else {
            return self.message("Rename", "The language server didn't answer in time.".into());
        };
        let start = self.active;
        let mut changed = 0;
        for (file, mut edits) in lsp::workspace_edits(&result) {
            let index = match self.docs.iter().position(|d| d.path.as_deref().is_some_and(|p| same_path(p, &file))) {
                Some(i) => i,
                None if self.open_path(&file) => self.active,
                None => continue,
            };
            // From the end, so the places before stay where they are.
            edits.sort_by_key(|e| std::cmp::Reverse((e.0, e.1)));
            let buffer = &mut self.docs[index].text;
            buffer.begin_undo_group(false);
            for (l1, u1, l2, u2, new_text) in edits {
                if l1 >= buffer.line_count() || l2 >= buffer.line_count() {
                    continue;
                }
                let (c1, c2) = (lsp::from_utf16(&buffer.line(l1), u1), lsp::from_utf16(&buffer.line(l2), u2));
                buffer.move_caret(l1, c1, false);
                buffer.move_caret(l2, c2, true);
                buffer.type_text(&new_text);
                changed += 1;
            }
            buffer.end_undo_group();
        }
        self.active = start;
        self.script_print(format!("Renamed to {new_name} in {changed} place{}.", if changed == 1 { "" } else { "s" }), true);
    }

    /// The box saying which command serves which language.
    pub(super) fn language_servers_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::LanguageServers(list) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut keep = false;
        egui::Window::new("Language servers").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label("Each language, as the highlighter names it (Format, Colour as), and the command that starts its server. A server is started for a file in that language, in its project's folder.");
            ui.add_space(6.0);
            let mut gone = None;
            egui::Grid::new("lsp-servers").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
                for (index, setting) in list.iter_mut().enumerate() {
                    ui.add(egui::TextEdit::singleline(&mut setting.language).desired_width(160.0));
                    ui.add(egui::TextEdit::singleline(&mut setting.command).desired_width(260.0));
                    let program = crate::emulators::split_options(&setting.command).into_iter().next().unwrap_or_default();
                    let found = lsp::on_path(&program);
                    ui.label(RichText::new(if found { "installed" } else { "not found" }).size(11.5).color(if found { theme::text_dim() } else { theme::text_faint() }));
                    if crate::access::called(ui.small_button("\u{D7}"), "Remove this language server").clicked() {
                        gone = Some(index);
                    }
                    ui.end_row();
                }
            });
            if let Some(index) = gone {
                list.remove(index);
            }
            ui.horizontal(|ui| {
                if ui.button("Add").clicked() {
                    list.push(ServerSetting { language: String::new(), command: String::new() });
                }
                if ui.button("Put back the usual list").clicked() {
                    *list = lsp::usual_servers();
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                keep = ui.button("OK").clicked();
                close |= ui.button("Cancel").clicked();
            });
        });
        if keep {
            if let Dialog::LanguageServers(list) = std::mem::replace(&mut self.dialog, Dialog::None) {
                self.lsp.settings = list.into_iter().filter(|s| !s.language.trim().is_empty() && !s.command.trim().is_empty()).collect();
                // Servers start again as they're now set up.
                self.lsp.stop_all();
                self.lsp.failed.clear();
            }
        } else if close {
            self.dialog = Dialog::None;
        }
        true
    }

    /// Whether the document in front is looked after by a language server.
    pub(super) fn has_lsp(&self) -> bool {
        !self.no_file() && self.lsp_server_for(self.active).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    /// A file looked after by a stand-in server: its problems are marked, words come to finish one being typed,
    /// hover says what's there, and F12, Shift+F12 and renaming go through the server.
    #[test]
    fn a_language_server_looks_after_a_file() {
        crate::highlight::load_now();
        let (mut app, _ctx) = app_with("");
        let folder = std::env::temp_dir().join(format!("limoniter-lsp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("lib.rs");
        std::fs::write(&file, "fn count_up(n: u32) -> u32 { n }\nlet x = oops;\nlet y = count_up(1);\n").unwrap();
        assert!(app.open_path(&file));
        app.doc().update_syntax();
        assert_eq!(app.doc().language_name(), "Rust");
        // The stand-in, as the server for Rust in this folder.
        app.lsp.on = true;
        let key = format!("rust-analyzer\u{1}{}", lsp::root_for(&file).display());
        app.lsp.servers.push((key, crate::lsp::tests::fake_server(&folder)));
        let settle = |app: &mut Limoniter| {
            for _ in 0..60 {
                app.lsp_tick();
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        settle(&mut app);
        assert_eq!(app.doc().text.intel.as_ref().map(|i| i.diagnostics.clone()), Some(vec![(1, "oops isn't allowed (fake)".to_string())]), "its problems are marked");
        // Typing a word asks for words to finish it.
        app.doc().text.move_caret(2, 0, false);
        app.doc().text_state.completion_wanted = Some((2, 10));
        settle(&mut app);
        let offered = app.doc().text.intel.as_ref().and_then(|i| i.lsp_completion.clone());
        assert_eq!(offered.map(|(l, start, words)| (l, start, words.len())), Some((2, 8, 2)), "words for the word starting at column 8");
        // Hover.
        app.doc().text_state.hover_at = Some((2, 10));
        app.lsp_tick();
        settle(&mut app);
        let intel = app.doc().text.intel.clone().unwrap();
        assert_eq!(intel.describe(2, "let y = count_up(1);", 10).as_deref(), Some("fn count_up(n: u32) -> u32"));
        // F12: the definition, as the stand-in says, on the first line.
        app.doc().text.move_caret(2, 10, false);
        app.go_to_definition();
        assert_eq!((app.doc().text.caret_line, app.doc().text.caret_col), (0, 3));
        app.doc().text.move_caret(2, 10, false);
        app.find_uses();
        assert!(app.script_output.iter().any(|l| l == "count_up: 2 places"), "{:?}", app.script_output);
        app.doc().text.move_caret(2, 10, false);
        app.lsp_rename("count_more");
        assert_eq!(&*app.doc().text.line(0), "fn count_more(n: u32) -> u32 { n }");
        assert_eq!(&*app.doc().text.line(2), "let y = count_more(1);");
        app.lsp.stop_all();
        let _ = std::fs::remove_dir_all(folder);
    }

    /// With LIMONITER_PYLSP naming a real Python language server: a Python file's mistake is marked, and F12 goes
    /// to a function's definition.
    #[test]
    #[ignore]
    fn a_real_python_server_looks_after_a_file() {
        let Ok(command) = std::env::var("LIMONITER_PYLSP") else {
            return;
        };
        crate::highlight::load_now();
        let (mut app, _ctx) = app_with("");
        let folder = std::env::temp_dir().join(format!("limoniter-pylsp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("tool.py");
        std::fs::write(&file, "def double(n):\n    return n * 2\n\nprint(double(4))\nprint(undefined_name)\n").unwrap();
        assert!(app.open_path(&file));
        app.doc().update_syntax();
        app.lsp.on = true;
        app.lsp.settings = vec![ServerSetting { language: "Python".into(), command }];
        let began = std::time::Instant::now();
        while began.elapsed() < Duration::from_secs(30) && app.doc().text.intel.as_ref().is_none_or(|i| i.diagnostics.is_empty()) {
            app.lsp_tick();
            std::thread::sleep(Duration::from_millis(50));
        }
        let problems = app.doc().text.intel.as_ref().map(|i| i.diagnostics.clone()).unwrap_or_default();
        assert!(problems.iter().any(|(line, words)| *line == 4 && words.contains("undefined_name")), "{problems:?}");
        app.doc().text.move_caret(3, 8, false);
        app.go_to_definition();
        assert_eq!(app.doc().text.caret_line, 0, "to def double");
        app.lsp.stop_all();
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn the_settings_are_kept() {
        let state = LspState::from_settings("lsp_on=true\n");
        assert!(state.on);
        assert_eq!(state.settings, lsp::usual_servers());
        assert_eq!(state.to_settings(), "lsp_on=true\n");
        let mut changed = state;
        changed.settings.push(ServerSetting { language: "Zig".into(), command: "zls".into() });
        let back = LspState::from_settings(&changed.to_settings());
        assert_eq!(back.settings.last().map(|s| s.command.as_str()), Some("zls"));
        assert!(!LspState::from_settings("").on, "off to begin with");
    }
}
