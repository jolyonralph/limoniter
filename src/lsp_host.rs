//! The window's side of language servers. With them switched on (Settings, they start off), the file in front is
//! looked after by the server for its language, started in its project's folder: told of the file and its changes
//! a moment after typing stops, asked for words to finish one being typed and about what's under the pointer, and
//! asked by F12, Shift+F12 and Ctrl+F12 for definitions, uses and renames. Its problems are marked by their lines.

use super::*;
use crate::lsp::{self, Server, ServerSetting};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};

/// What's been asked of a server and not yet answered.
enum Pending {
    /// Words for a place: the document, the line, and where the word starts.
    Completion(u64, usize, usize),
    /// About a place: the document, line and column.
    Hover(u64, usize, usize),
    /// The call being typed: the document and line.
    Signature(u64, usize),
    /// Inline hints for a document, at a version.
    Hints(u64, u64),
    /// A command run, whose answer doesn't matter (what it changes comes as edits asked for).
    Ignored,
}

/// One edit to a file: (line, from, to, text), with the columns in UTF-16 as servers count them.
type Edit = (usize, usize, usize, usize, String);

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
    /// Files formatted by their server as they're saved.
    pub format_on_save: bool,
    /// The types and parameters' names a server can show at the ends of lines.
    pub hints: bool,
    /// The place signature help was last asked for: document, version, line and column.
    signature_asked: Option<(u64, u64, usize, usize)>,
    /// The fixes offered for the cursor's line: the server, each fix's title and itself, and the one chosen.
    pub fixes: Option<(usize, Vec<(String, Value)>, usize)>,
    /// A server offered for the file in front: its language, its program, and how to install it if it isn't.
    pub offer: Option<Offer>,
    /// The document last thought about offering one for; the languages offered this session; and those never to be.
    considered: Option<u64>,
    offered: BTreeSet<String>,
    pub never: BTreeSet<String>,
    /// A server being installed in a terminal: its language and program, whether the program was there before,
    /// and when it was last looked for.
    pub installing: Option<(String, String, bool, std::time::Instant)>,
}

/// A language server offered for a language.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Offer {
    pub language: String,
    pub program: String,
    /// The command that installs it, if it's missing (or didn't start); None if it's there to be used.
    pub install: Option<String>,
}

impl LspState {
    pub fn from_settings(settings: &str) -> Self {
        let on = list_setting(settings, "lsp_on").first().is_some_and(|v| v == "true");
        let servers = if list_setting(settings, "lsp_servers").iter().any(|v| v == "changed") {
            list_setting(settings, "language_server").iter().filter_map(|v| v.split_once('\u{1}')).map(|(l, c)| ServerSetting { language: l.into(), command: c.into() }).collect()
        } else {
            lsp::usual_servers()
        };
        let format_on_save = list_setting(settings, "lsp_format_on_save").first().is_some_and(|v| v == "true");
        let hints = list_setting(settings, "lsp_hints").first().is_none_or(|v| v == "true");
        let never = list_setting(settings, "lsp_never").into_iter().collect();
        LspState { on, settings: servers, format_on_save, hints, never, ..Default::default() }
    }

    pub fn to_settings(&self) -> String {
        let mut text = format!("lsp_on={}\n", self.on);
        if self.format_on_save {
            text.push_str("lsp_format_on_save=true\n");
        }
        if !self.hints {
            text.push_str("lsp_hints=false\n");
        }
        for language in &self.never {
            text.push_str(&format!("lsp_never={language}\n"));
        }
        if self.settings != lsp::usual_servers() {
            text.push_str("lsp_servers=changed\n");
            for s in &self.settings {
                text.push_str(&format!("language_server={}\u{1}{}\n", s.language, s.command));
            }
        }
        text
    }

    /// Thinks again about offering a server for the file in front, as extensions have changed what there is.
    pub fn reconsider(&mut self) {
        self.considered = None;
    }

    /// The server for a language: as Settings has it, or else one a VS Code extension gives.
    pub fn server_for(&self, language: &str) -> Option<ServerSetting> {
        self.settings.iter().find(|s| s.language == language).cloned().or_else(|| crate::vsix::server_for(language).map(|command| ServerSetting { language: language.to_string(), command }))
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
    pub(super) fn lsp_server_for(&self, index: usize) -> Option<(usize, PathBuf)> {
        let doc = self.docs.get(index)?;
        let (server, _, _) = self.lsp.opened.get(&doc.id)?;
        Some((*server, doc.path.clone()?))
    }

    /// Whether to offer a language server for the file in front: one that's installed, while they're off; or, to
    /// install, one that isn't there or didn't start. Once a session for each language, and never for those
    /// turned down for good.
    fn consider_offering_lsp(&mut self) {
        let Some(doc) = self.docs.get(self.active).filter(|d| d.path.is_some() && !d.hex_mode && d.browser.is_none()) else { return };
        if self.lsp.considered == Some(doc.id) || self.lsp.installing.is_some() {
            return;
        }
        self.lsp.considered = Some(doc.id);
        let language = doc.language_name();
        if self.lsp.offered.contains(&language) || self.lsp.never.contains(&language) {
            self.lsp.offer = None;
            return;
        }
        let Some(setting) = self.lsp.server_for(&language) else {
            self.lsp.offer = None;
            return;
        };
        let program = crate::emulators::split_options(&setting.command).into_iter().next().unwrap_or_default();
        let there = lsp::on_path(&program);
        let failed = self.lsp.failed.contains(&language);
        self.lsp.offer = if there && !failed {
            // A server written in JavaScript is shown by its own name, not Node.js's.
            let words = crate::emulators::split_options(&setting.command);
            let program = match words.get(1).filter(|_| program == "node") {
                Some(script) => format!("{} (Node.js)", Path::new(script).file_name().map_or_else(|| script.clone(), |n| n.to_string_lossy().into_owned())),
                None => program,
            };
            (!self.lsp.on).then_some(Offer { language, program, install: None })
        } else {
            lsp::install_command(&program).map(|(command, _)| Offer { language, program, install: Some(command) })
        };
    }

    /// The bar offering a language server for the file in front, or saying one is being installed.
    pub(super) fn lsp_offer_bar(&mut self, ui: &mut Ui) {
        self.consider_offering_lsp();
        // While one's installed, it's looked for now and then, and started once it's there.
        if let Some((language, program, was_there, when)) = self.lsp.installing.clone() {
            if when.elapsed() >= Duration::from_secs(2) {
                self.lsp.installing = Some((language.clone(), program.clone(), was_there, std::time::Instant::now()));
                if !was_there && lsp::on_path(&program) {
                    self.lsp_start_again(&language);
                    self.script_print(crate::i18n::tf!("{program} is installed, and looks after {language} files now."), true);
                    return;
                }
            }
            let (mut start, mut stop) = (false, false);
            Panel::top("lsp-installing").frame(egui::Frame::new().fill(theme::selected()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::i18n::tf!("Installing {program} in the terminal. When it's finished:"));
                    start = ui.button(crate::i18n::tr("Start it")).clicked();
                    stop = ui.button(crate::i18n::tr("Never mind")).clicked();
                });
            });
            if start {
                self.lsp_start_again(&language);
            } else if stop {
                self.lsp.installing = None;
            }
            return;
        }
        let Some(offer) = self.lsp.offer.clone() else { return };
        let (mut yes, mut later, mut never) = (false, false, false);
        Panel::top("lsp-offer").frame(egui::Frame::new().fill(theme::selected()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let (language, program) = (&offer.language, &offer.program);
                let help = crate::i18n::tr("A language server knows the language: it marks mistakes as you type, finishes words, says what things are, goes to where they're defined, renames them everywhere, and lays out and fixes code.");
                match &offer.install {
                    None => {
                        ui.label(RichText::new(crate::i18n::tf!("{program}, a language server for {language}, is installed. Use it?")).strong()).on_hover_text(help);
                        yes = ui.button(crate::i18n::tr("Use it")).clicked();
                    }
                    Some(command) => {
                        ui.label(RichText::new(crate::i18n::tf!("{language} files can have a language server, {program}, which isn't installed. Install it?")).strong()).on_hover_text(help);
                        yes = ui.button(crate::i18n::tr("Install it in a terminal")).on_hover_text(command.as_str()).clicked();
                    }
                }
                later = ui.button(crate::i18n::tr("Not now")).clicked();
                never = ui.button(crate::i18n::tf!("Never for {}", offer.language)).clicked();
            });
        });
        if yes || later || never {
            self.lsp.offered.insert(offer.language.clone());
            self.lsp.offer = None;
        }
        if never {
            self.lsp.never.insert(offer.language.clone());
            self.save_settings_if_changed();
        }
        if yes {
            match offer.install {
                None => {
                    self.lsp.on = true;
                    self.lsp.failed.remove(&offer.language);
                    self.save_settings_if_changed();
                }
                Some(command) => {
                    let was_there = lsp::on_path(&offer.program);
                    self.new_terminal(None);
                    if let Some(terminal) = self.terminal.as_mut() {
                        terminal.send(format!("{command}\r").as_bytes());
                    }
                    self.lsp.installing = Some((offer.language, offer.program, was_there, std::time::Instant::now()));
                }
            }
        }
    }

    /// Starts the servers afresh, with language servers on, after one's been installed.
    fn lsp_start_again(&mut self, language: &str) {
        self.lsp.installing = None;
        self.lsp.stop_all();
        self.lsp.failed.remove(language);
        self.lsp.on = true;
        self.save_settings_if_changed();
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
        // A server that's ended is said so, with why, and not used again this session.
        let mut ended = Vec::new();
        for (index, (_, server)) in self.lsp.servers.iter_mut().enumerate() {
            if !server.dead
                && let Some(why) = server.ended()
            {
                server.dead = true;
                ended.push((index, server.name.clone(), why));
            }
        }
        for (index, name, why) in ended {
            self.lsp.pending.retain(|(s, ..)| *s != index);
            let languages: Vec<String> = self.lsp.opened.values().filter(|(s, ..)| *s == index).filter_map(|(_, _, p)| self.docs.iter().find(|d| d.path.as_deref() == Some(p.as_path())).map(|d| d.language_name())).collect();
            self.lsp.failed.extend(languages);
            self.lsp.considered = None;
            self.script_print(if why.is_empty() { crate::i18n::tf!("The language server {name} has stopped.") } else { crate::i18n::tf!("The language server {name} has stopped: {why}") }, true);
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
        !self.lsp.pending.is_empty() || self.lsp.servers.iter().any(|(_, s)| !s.ready() && !s.dead)
    }

    fn lsp_follow(&mut self, index: usize) {
        let doc = &self.docs[index];
        let Some(path) = doc.path.clone().filter(|_| !doc.hex_mode && !doc.text.is_disk_backed() && doc.browser.is_none()) else {
            return;
        };
        let language = doc.language_name();
        let Some(setting) = self.lsp.server_for(&language) else {
            return;
        };
        if self.lsp.failed.contains(&language) {
            return;
        }
        let root = lsp::root_for(&path);
        let key = format!("{}\u{1}{}", setting.command, root.display());
        let server = match self.lsp.servers.iter().position(|(k, _)| *k == key) {
            Some(i) if self.lsp.servers[i].1.dead => {
                self.lsp.failed.insert(language);
                return;
            }
            Some(i) => i,
            None => match Server::start(&setting.command, &root) {
                Ok(server) => {
                    self.script_print(crate::i18n::tf!("Started {} for {language}, in {}", setting.command, root.display()), false);
                    self.lsp.servers.push((key, server));
                    self.lsp.servers.len() - 1
                }
                Err(problem) => {
                    self.lsp.failed.insert(language.clone());
                    self.script_print(crate::i18n::tf!("{problem}. It's in Settings, Editing, Language servers."), false);
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
        // Inside a call's brackets, what it takes, with the parameter being typed picked out.
        let doc = &mut self.docs[index];
        let (line, col) = (doc.text.caret_line, doc.text.caret_col);
        let before: String = doc.text.line(line).chars().take(col).collect();
        let depth = before.matches('(').count() as i64 - before.matches(')').count() as i64;
        let intel = doc.text.intel.get_or_insert_with(Box::default);
        if intel.lsp_signature.as_ref().is_some_and(|(l, ..)| *l != line) || depth <= 0 {
            intel.lsp_signature = None;
        }
        if depth > 0 && self.lsp.servers[server].1.can("signatureHelpProvider") && self.lsp.signature_asked != Some((doc_id, version, line, col)) && self.lsp.opened.get(&doc_id).is_some_and(|(_, v, _)| *v == version) {
            self.lsp.signature_asked = Some((doc_id, version, line, col));
            let id = self.lsp.servers[server].1.at("textDocument/signatureHelp", &path, line, lsp::to_utf16(&before, col));
            self.lsp.pending.push((server, id, Pending::Signature(doc_id, line)));
        }
        // Inline hints, for the whole file, once the server has its latest text.
        let doc = &self.docs[index];
        if self.lsp.hints
            && self.lsp.servers[server].1.can("inlayHintProvider")
            && doc.text.intel.as_ref().is_some_and(|i| i.lsp_hints_version != version)
            && self.lsp.opened.get(&doc_id).is_some_and(|(_, v, _)| *v == version)
            && !self.lsp.pending.iter().any(|(_, _, p)| matches!(p, Pending::Hints(d, v) if *d == doc_id && *v == version))
        {
            let last = doc.text.line_count().saturating_sub(1);
            let end = lsp::to_utf16(&doc.text.line(last), doc.text.line(last).chars().count());
            let id = self.lsp.servers[server].1.request("textDocument/inlayHint", json!({ "textDocument": { "uri": lsp::uri(&path) }, "range": { "start": { "line": 0, "character": 0 }, "end": { "line": last, "character": end } } }));
            self.lsp.pending.push((server, id, Pending::Hints(doc_id, version)));
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
                    intel.lsp_problems = message.pointer("/params/diagnostics").and_then(Value::as_array).cloned().unwrap_or_default();
                }
                continue;
            }
            // The server's hints have changed (it's read the project, say): they're asked for again.
            if message.get("method").and_then(Value::as_str) == Some("workspace/inlayHint/refresh") {
                for (doc_id, (s, ..)) in &self.lsp.opened {
                    if *s == server
                        && let Some(intel) = self.docs.iter_mut().find(|d| d.id == *doc_id).and_then(|d| d.text.intel.as_mut())
                    {
                        intel.lsp_hints_version = u64::MAX;
                    }
                }
                if let Some(id) = message.get("id").cloned() {
                    self.lsp.servers[server].1.respond(id, Value::Null);
                }
                continue;
            }
            // An edit the server wants made, as a fix's command asks for: made, and said so.
            if message.get("method").and_then(Value::as_str) == Some("workspace/applyEdit") {
                let changed = message.pointer("/params/edit").map_or(0, |edit| self.lsp_apply_workspace_edit(edit));
                if let Some(id) = message.get("id").cloned() {
                    self.lsp.servers[server].1.respond(id, json!({ "applied": changed > 0 }));
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
                Pending::Signature(doc_id, line) => {
                    if let Some(doc) = self.docs.iter_mut().find(|d| d.id == doc_id)
                        && doc.text.caret_line == line
                        && let Some(intel) = doc.text.intel.as_mut()
                    {
                        intel.lsp_signature = lsp::signature(&result).map(|(words, part)| (line, words, part));
                    }
                }
                Pending::Hints(doc_id, version) => {
                    if let Some(intel) = self.docs.iter_mut().find(|d| d.id == doc_id).and_then(|d| d.text.intel.as_mut()) {
                        intel.lsp_hints = lsp::inlay_hints(&result);
                        intel.lsp_hints_version = version;
                    }
                }
                Pending::Ignored => {}
            }
        }
    }

    /// Tells a document's server its latest text, if it hasn't been told; its server and path.
    fn lsp_sync(&mut self, index: usize) -> Option<(usize, PathBuf)> {
        let (server, path) = self.lsp_server_for(index)?;
        let doc = &self.docs[index];
        let version = doc.text.version();
        if self.lsp.opened.get(&doc.id).is_some_and(|(_, v, _)| *v != version) {
            let whole = whole_text(doc);
            self.lsp.servers[server].1.did_change(&path, version, &whole);
            self.lsp.opened.insert(doc.id, (server, version, path.clone()));
        }
        Some((server, path))
    }

    /// Makes edits to a document, from the last back so the places before stay where they are; how many were made.
    fn lsp_apply_edits(&mut self, index: usize, mut edits: Vec<Edit>) -> usize {
        edits.sort_by_key(|e| std::cmp::Reverse((e.0, e.1)));
        let buffer = &mut self.docs[index].text;
        let last = buffer.line_count().saturating_sub(1);
        // A place past the end (as a whole-file edit ends) is the end.
        let place = |buffer: &crate::text_buffer::TextBuffer, line: usize, units: usize| if line > last { (last, buffer.line(last).chars().count()) } else { (line, lsp::from_utf16(&buffer.line(line), units)) };
        let mut changed = 0;
        buffer.begin_undo_group(false);
        for (l1, u1, l2, u2, new_text) in edits {
            let ((l1, c1), (l2, c2)) = (place(buffer, l1, u1), place(buffer, l2, u2));
            buffer.move_caret(l1, c1, false);
            buffer.move_caret(l2, c2, true);
            if buffer.selection_text() == new_text {
                continue;
            }
            buffer.type_text(&new_text);
            changed += 1;
        }
        buffer.end_undo_group();
        changed
    }

    /// Makes a server's edits to files, opening those that aren't; how many were made.
    fn lsp_apply_workspace_edit(&mut self, edit: &Value) -> usize {
        let start = self.active;
        let mut changed = 0;
        for (file, edits) in lsp::workspace_edits(edit) {
            let index = match self.docs.iter().position(|d| d.path.as_deref().is_some_and(|p| same_path(p, &file))) {
                Some(i) => i,
                None if self.open_path(&file) => self.active,
                None => continue,
            };
            changed += self.lsp_apply_edits(index, edits);
        }
        self.active = start.min(self.docs.len().saturating_sub(1));
        changed
    }

    /// Has the server format a document, as it's saved (quietly) or when asked.
    pub(super) fn lsp_format(&mut self, index: usize, quiet: bool) {
        let Some((server, path)) = self.lsp_sync(index) else {
            // No language server: a formatter installed for the language, if there is one.
            if self.external_format(index, quiet) {
                return;
            }
            if !quiet {
                self.message("Format document", crate::i18n::tr("Formatting is done by a language server for the file's language (Settings, Editing, Language servers), or by a formatter such as Prettier, Black or clang-format when one is installed (Format, Formatters).").into());
            }
            return;
        };
        if !self.lsp.servers[server].1.can("documentFormattingProvider") {
            if self.external_format(index, quiet) {
                return;
            }
            if !quiet {
                let name = self.lsp.servers[server].1.name.clone();
                self.message("Format document", crate::i18n::tf!("The language server ({name}) doesn't format files."));
            }
            return;
        }
        let spaces = self.editor_config.as_ref().and_then(|(_, c)| c.spaces).unwrap_or(self.prefs.edit.spaces_for_tab);
        let options = json!({ "tabSize": crate::text_buffer::tab_size(), "insertSpaces": spaces, "trimTrailingWhitespace": true, "insertFinalNewline": true });
        let server_ref = &mut self.lsp.servers[server].1;
        let id = server_ref.request("textDocument/formatting", json!({ "textDocument": { "uri": lsp::uri(&path) }, "options": options }));
        match server_ref.wait_for(id, Duration::from_secs(if quiet { 2 } else { 5 })) {
            Some(result) => {
                let (line, col) = (self.docs[index].text.caret_line, self.docs[index].text.caret_col);
                self.lsp_apply_edits(index, lsp::text_edits(&result));
                let buffer = &mut self.docs[index].text;
                let line = line.min(buffer.line_count().saturating_sub(1));
                buffer.move_caret(line, col, false);
            }
            None if !quiet => self.message("Format document", crate::i18n::tr("The language server didn't answer in time.").into()),
            None => {}
        }
    }

    /// Asks the server what it can fix or change on the cursor's line, and offers it in a list (Ctrl+.).
    pub(super) fn lsp_quick_fix(&mut self) {
        let Some((server, path)) = self.lsp_sync(self.active) else {
            return self.message("Quick fix", crate::i18n::tr("Fixes come from a language server for the file's language (Settings, Editing, Language servers).").into());
        };
        let doc = &self.docs[self.active];
        let line = doc.text.caret_line;
        let text = doc.text.line(line).to_string();
        let problems: Vec<Value> = doc.text.intel.as_ref().map(|i| i.lsp_problems.iter().filter(|p| p.pointer("/range/start/line").and_then(Value::as_u64).is_some_and(|l| l as usize <= line) && p.pointer("/range/end/line").and_then(Value::as_u64).is_some_and(|l| l as usize >= line)).cloned().collect()).unwrap_or_default();
        let (from, to) = if doc.text.has_selection() {
            let ((l1, c1), (l2, c2)) = doc.text.selection_char_bounds();
            ((l1, lsp::to_utf16(&doc.text.line(l1), c1)), (l2, lsp::to_utf16(&doc.text.line(l2), c2)))
        } else {
            ((line, 0), (line, lsp::to_utf16(&text, text.chars().count())))
        };
        let server_ref = &mut self.lsp.servers[server].1;
        let id = server_ref.request("textDocument/codeAction", json!({ "textDocument": { "uri": lsp::uri(&path) }, "range": { "start": { "line": from.0, "character": from.1 }, "end": { "line": to.0, "character": to.1 } }, "context": { "diagnostics": problems } }));
        let Some(result) = server_ref.wait_for(id, Duration::from_secs(4)) else {
            return self.message("Quick fix", crate::i18n::tr("The language server didn't answer in time.").into());
        };
        let fixes = lsp::code_actions(&result);
        if fixes.is_empty() {
            return self.message("Quick fix", crate::i18n::tr("The language server has nothing to offer here.").into());
        }
        self.lsp.fixes = Some((server, fixes, 0));
    }

    /// Does a fix chosen from the list: its edit, or (asked for when it hasn't one yet) its command.
    fn lsp_do_fix(&mut self, server: usize, fix: Value) {
        let mut fix = fix;
        if fix.get("edit").is_none() && fix.get("command").is_none_or(|c| !c.is_string()) && fix.get("data").is_some() {
            let server_ref = &mut self.lsp.servers[server].1;
            let id = server_ref.request("codeAction/resolve", fix.clone());
            if let Some(resolved) = server_ref.wait_for(id, Duration::from_secs(4)) {
                fix = resolved;
            }
        }
        if let Some(edit) = fix.get("edit") {
            self.lsp_apply_workspace_edit(&edit.clone());
        }
        // A bare command, or a fix's own; what it changes comes back as edits the server asks for.
        let command = match fix.get("command") {
            Some(Value::String(name)) => Some(json!({ "command": name, "arguments": fix.get("arguments").cloned().unwrap_or(json!([])) })),
            Some(command @ Value::Object(_)) => Some(json!({ "command": command.get("command").cloned().unwrap_or(Value::Null), "arguments": command.get("arguments").cloned().unwrap_or(json!([])) })),
            _ => None,
        };
        if let Some(command) = command {
            let id = self.lsp.servers[server].1.request("workspace/executeCommand", command);
            self.lsp.pending.push((server, id, Pending::Ignored));
        }
    }

    /// The list of fixes offered: arrows choose, Enter or a click does one, Escape closes it.
    pub(super) fn quick_fix_window(&mut self, ctx: &Context) {
        let Some((server, fixes, mut chosen)) = self.lsp.fixes.take() else { return };
        let (mut close, mut take) = (false, None);
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                chosen = (chosen + 1).min(fixes.len() - 1);
            }
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                chosen = chosen.saturating_sub(1);
            }
            if i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                close = true;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::Enter) {
                take = Some(chosen);
            }
        });
        egui::Window::new(crate::i18n::tr("Quick fix")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 80.0]).show(ctx, |ui| {
            ui.set_min_width(360.0);
            for (n, (title, _)) in fixes.iter().enumerate() {
                if ui.selectable_label(n == chosen, title.as_str()).clicked() {
                    take = Some(n);
                }
            }
        });
        if let Some(n) = take {
            self.lsp_do_fix(server, fixes[n].1.clone());
            self.pending_focus = true;
        } else if close {
            self.pending_focus = true;
        } else {
            self.lsp.fixes = Some((server, fixes, chosen));
        }
    }

    /// Sends a request to a server and waits for its answer, up to a few seconds.
    pub(super) fn lsp_request_wait(&mut self, server: usize, method: &str, params: Value) -> Option<Value> {
        let server_ref = &mut self.lsp.servers.get_mut(server)?.1;
        let id = server_ref.request(method, params);
        server_ref.wait_for(id, Duration::from_secs(3))
    }

    /// Asks the server about the word at the cursor, waiting for its answer a few seconds.
    pub(super) fn lsp_ask_here(&mut self, method: &str, what: &str) -> Option<(Value, PathBuf)> {
        let Some((server, path)) = self.lsp_sync(self.active) else {
            self.message(what, crate::i18n::tr("This works in assembler source, and with a language server for the file's language (Settings, Editing, Language servers).").into());
            return None;
        };
        let doc = &self.docs[self.active];
        let (line, col) = (doc.text.caret_line, doc.text.caret_col);
        let text = doc.text.line(line).to_string();
        let server_ref = &mut self.lsp.servers[server].1;
        let id = server_ref.at(method, &path, line, lsp::to_utf16(&text, col));
        match server_ref.wait_for(id, Duration::from_secs(4)) {
            Some(result) => Some((result, path)),
            None => {
                let name = self.lsp.servers[server].1.name.clone();
                self.message(what, crate::i18n::tf!("The language server ({name}) didn't answer in time. It may still be reading the project; try again in a moment."));
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
            return self.message("Go to definition", crate::i18n::tr("The language server doesn't know where that's defined.").into());
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
        self.script_print(if places.len() == 1 { crate::i18n::tf!("{word}: {} place", places.len()) } else { crate::i18n::tf!("{word}: {} places", places.len()) }, true);
        for place in places {
            let text = self.line_of_path(&place.path, place.line);
            self.script_print(crate::i18n::tf!("{}({}): {}", place.path.display(), place.line + 1, text.trim()), false);
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
            return self.message("Rename", crate::i18n::tr("The language server didn't answer in time.").into());
        };
        let changed = self.lsp_apply_workspace_edit(&result);
        self.script_print(if changed == 1 { crate::i18n::tf!("Renamed to {new_name} in {changed} place.") } else { crate::i18n::tf!("Renamed to {new_name} in {changed} places.") }, true);
    }

    /// The box saying which command serves which language.
    pub(super) fn language_servers_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::LanguageServers(list) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut keep = false;
        egui::Window::new(crate::i18n::tr("Language servers")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(crate::i18n::tr("Each language, as the highlighter names it (Format, Colour as), and the command that starts its server. A server is started for a file in that language, in its project's folder."));
            ui.add_space(6.0);
            let mut gone = None;
            egui::Grid::new("lsp-servers").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
                for (index, setting) in list.iter_mut().enumerate() {
                    ui.add(egui::TextEdit::singleline(&mut setting.language).desired_width(160.0));
                    ui.add(egui::TextEdit::singleline(&mut setting.command).desired_width(260.0));
                    let program = crate::emulators::split_options(&setting.command).into_iter().next().unwrap_or_default();
                    let found = lsp::on_path(&program);
                    ui.label(RichText::new(if found { crate::i18n::tr("installed") } else { crate::i18n::tr("not found") }).size(11.5).color(if found { theme::text_dim() } else { theme::text_faint() }));
                    if crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Remove this language server").clicked() {
                        gone = Some(index);
                    }
                    ui.end_row();
                }
            });
            if let Some(index) = gone {
                list.remove(index);
            }
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("Add")).clicked() {
                    list.push(ServerSetting { language: String::new(), command: String::new() });
                }
                if ui.button(crate::i18n::tr("Put back the usual list")).clicked() {
                    *list = lsp::usual_servers();
                }
            });
            ui.add_space(4.0);
            ui.checkbox(&mut self.lsp.format_on_save, crate::i18n::tr("Format files as they're saved, where the server can"));
            ui.checkbox(&mut self.lsp.hints, crate::i18n::tr("Show the types and parameters' names a server gives, at the ends of lines"));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                keep = ui.button(crate::i18n::tr("OK")).clicked();
                close |= ui.button(crate::i18n::tr("Cancel")).clicked();
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
        // Inline hints come for the file once the server has its text.
        settle(&mut app);
        assert_eq!(app.doc().text.intel.as_ref().map(|i| i.lsp_hints.clone()), Some(vec![(2, ": u32".to_string())]));
        // Inside a call's brackets, its signature, with the parameter picked out.
        app.doc().text.move_caret(2, 19, false);
        settle(&mut app);
        let signature = app.doc().text.intel.as_ref().and_then(|i| i.lsp_signature.clone());
        assert_eq!(signature, Some((2, "fn count_up(n: u32) -> u32".to_string(), Some((12, 18)))));
        // Out of them, it goes.
        app.doc().text.move_caret(2, 21, false);
        settle(&mut app);
        assert!(app.doc().text.intel.as_ref().is_some_and(|i| i.lsp_signature.is_none()));
        // Format document.
        app.doc().text.move_caret(1, 0, false);
        app.doc().text.type_text("let  z = 1;\n");
        app.lsp_format(app.active, false);
        assert_eq!(&*app.doc().text.line(1), "let z = 1;");
        // Quick fixes: one an edit, made at once; one a command, whose edit the server asks for.
        app.doc().text.move_caret(2, 9, false);
        app.lsp_quick_fix();
        let titles: Vec<String> = app.lsp.fixes.as_ref().map(|(_, f, _)| f.iter().map(|(t, _)| t.clone()).collect()).unwrap_or_default();
        assert_eq!(titles, ["Change oops to 0", "Add a comment"]);
        let (server, fixes, _) = app.lsp.fixes.take().unwrap();
        app.lsp_do_fix(server, fixes[0].1.clone());
        assert_eq!(&*app.doc().text.line(2), "let x = 0;");
        app.lsp_do_fix(server, fixes[1].1.clone());
        settle(&mut app);
        assert_eq!(&*app.doc().text.line(0), "// counts");
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
        // Inside double's brackets, its signature.
        app.doc().text.move_caret(3, 13, false);
        let began = std::time::Instant::now();
        while began.elapsed() < Duration::from_secs(20) && app.doc().text.intel.as_ref().is_none_or(|i| i.lsp_signature.is_none()) {
            app.lsp_tick();
            std::thread::sleep(Duration::from_millis(50));
        }
        let signature = app.doc().text.intel.as_ref().and_then(|i| i.lsp_signature.clone());
        assert!(signature.as_ref().is_some_and(|(line, words, part)| *line == 3 && words.contains("double(n)") && part.is_some()), "{signature:?}");
        app.lsp.stop_all();
        let _ = std::fs::remove_dir_all(folder);
    }

    /// A server whose program ends at once is said to have stopped, with what it said, and isn't waited for.
    #[cfg(unix)]
    #[test]
    fn a_server_that_ends_says_why() {
        crate::highlight::load_now();
        let (mut app, _ctx) = app_with("");
        let folder = std::env::temp_dir().join(format!("limoniter-lsp-dies-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("tool.py");
        std::fs::write(&file, "print(1)\n").unwrap();
        assert!(app.open_path(&file));
        app.doc().update_syntax();
        app.lsp.on = true;
        app.lsp.settings = vec![ServerSetting { language: "Python".into(), command: "sh -c \"echo 'not installed for this toolchain' >&2; exit 1\"".into() }];
        let began = std::time::Instant::now();
        while began.elapsed() < Duration::from_secs(5) && !app.script_output.iter().any(|l| l.contains("has stopped")) {
            app.lsp_tick();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.script_output.iter().any(|l| l == "The language server sh has stopped: not installed for this toolchain"), "{:?}", app.script_output);
        assert!(!app.lsp_tick(), "nothing is waited for");
        let _ = std::fs::remove_dir_all(folder);
    }

    /// A Python file, with pylsp installed and servers off, has it offered; Never for Python is kept. One that
    /// didn't start is offered to be installed.
    #[test]
    fn a_language_server_is_offered() {
        if !lsp::on_path("pylsp") {
            return;
        }
        crate::highlight::load_now();
        let (mut app, ctx) = app_with("");
        let file = std::env::temp_dir().join(format!("limoniter-offer-{}.py", std::process::id()));
        std::fs::write(&file, "print(1)\n").unwrap();
        assert!(app.open_path(&file));
        app.doc().update_syntax();
        app.lsp.on = false;
        let _ = ctx.run_ui(Default::default(), |ui| app.lsp_offer_bar(ui));
        assert_eq!(app.lsp.offer, Some(Offer { language: "Python".into(), program: "pylsp".into(), install: None }));
        app.lsp.never.insert("Python".into());
        assert!(LspState::from_settings(&app.lsp.to_settings()).never.contains("Python"));
        // Once it's failed to start: offered to install, unless never.
        app.lsp.never.clear();
        app.lsp.failed.insert("Python".into());
        app.lsp.considered = None;
        let _ = ctx.run_ui(Default::default(), |ui| app.lsp_offer_bar(ui));
        assert!(app.lsp.offer.as_ref().is_some_and(|o| o.install.as_deref().is_some_and(|c| c.contains("python-lsp-server"))), "{:?}", app.lsp.offer);
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn the_settings_are_kept() {
        let state = LspState::from_settings("lsp_on=true\n");
        assert!(state.on);
        assert_eq!(state.settings, lsp::usual_servers());
        assert_eq!(state.to_settings(), "lsp_on=true\n");
        assert!(state.hints && !state.format_on_save);
        let both = LspState::from_settings("lsp_on=true\nlsp_format_on_save=true\nlsp_hints=false\n");
        assert!(both.format_on_save && !both.hints);
        assert_eq!(LspState::from_settings(&both.to_settings()).to_settings(), both.to_settings());
        let mut changed = state;
        changed.settings.push(ServerSetting { language: "Zig".into(), command: "zls".into() });
        let back = LspState::from_settings(&changed.to_settings());
        assert_eq!(back.settings.last().map(|s| s.command.as_str()), Some("zls"));
        assert!(!LspState::from_settings("").on, "off to begin with");
    }
}
