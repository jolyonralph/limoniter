//! The window's side of being Claude Code's IDE: starting the server (claude_ide.rs) when Claude Code is
//! installed, answering what a connected `claude` asks (a diff to review, the problems in a file), and telling
//! it what's selected and what to mention.

use super::*;
use crate::claude_ide::{self, Debounce, Event, PathStyle, Selection, Server};
use crate::review::Decision;
use std::sync::mpsc::{Receiver, Sender};

/// How long a selection must stay as it is before Claude Code is told of it.
const SELECTION_DELAY: Duration = Duration::from_millis(300);
/// How soon after a `claude` connects it's told what's selected.
const FIRST_SELECTION: Duration = Duration::from_millis(500);
/// The most text of a selection that's sent along.
const MOST_SELECTED: usize = 1_000_000;

/// A diff Claude Code asked to be shown, waiting for the person's decision.
struct OpenReview {
    client: u64,
    tab_name: String,
    tab: u64,
    decisions: Receiver<Decision>,
    reply: Sender<Vec<String>>,
}

pub(super) struct State {
    /// The server being started on a thread of its own, which says when it's ready (or why not).
    pub(super) starting: Option<Receiver<Result<Server, String>>>,
    pub server: Option<Server>,
    /// Starting has been tried, so it isn't again until the switch is turned off and on.
    tried: bool,
    reviews: Vec<OpenReview>,
    selection: Debounce<Selection>,
    /// What the selection was worked out from, so it's worked out again only when that changes.
    pub(super) selection_key: Option<(u64, u64, usize, usize, usize, usize)>,
    folders: Vec<String>,
}

impl Default for State {
    fn default() -> Self {
        Self { starting: None, server: None, tried: false, reviews: Vec::new(), selection: Debounce::new(SELECTION_DELAY), selection_key: None, folders: Vec::new() }
    }
}

impl Limoniter {
    /// The folders of the project, as the lock file lists them.
    fn claude_folders(&self) -> Vec<String> {
        let folder = self
            .project
            .as_ref()
            .and_then(|project| project.root.clone().or_else(|| project.path.parent().map(Path::to_path_buf)))
            .or_else(|| self.docs.get(self.active).and_then(|doc| doc.path.as_deref()).and_then(Path::parent).map(Path::to_path_buf));
        folder.into_iter().map(|folder| folder.display().to_string()).collect()
    }

    /// Whether a `claude` is connected now.
    pub(super) fn claude_connected(&self) -> bool {
        self.claude_ide.server.as_ref().is_some_and(|server| server.clients() > 0)
    }

    /// Starts the server, now, with the options given (the window starts it on a thread, and tests start it
    /// themselves, with no lock file in the person's folder).
    pub(super) fn claude_ide_use(&mut self, server: Server) {
        self.claude_ide.tried = true;
        claude_ide::publish_port(Some(server.port));
        self.claude_ide.server = Some(server);
    }

    /// Starts the server on a thread, when Claude Code can be used here and it's wanted.
    fn claude_ide_start(&mut self, ctx: &Context) {
        let folders = self.claude_folders();
        let ctx = ctx.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.claude_ide.tried = true;
        self.claude_ide.starting = Some(rx);
        std::thread::spawn(move || {
            let usable = assistant::installed("claude") || (cfg!(windows) && crate::terminal::shells().iter().any(|shell| matches!(shell, crate::terminal::Shell::Wsl(_))));
            if !usable {
                let _ = tx.send(Err(String::new()));
                return;
            }
            let also_on = if cfg!(windows) { wsl_gateway() } else { Vec::new() };
            let wake = ctx.clone();
            let options = claude_ide::Options { lock_dir: claude_ide::lock_dir(), folders, running_in_windows: cfg!(windows), also_on, wake: Arc::new(move || wake.request_repaint()) };
            let _ = tx.send(Server::start(options).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
    }

    /// Everything the window does for the server, each time round.
    pub(super) fn claude_ide_tick(&mut self, ctx: &Context) {
        // Turned off: the server goes, and its lock file with it.
        if !self.prefs.claude_ide {
            if self.claude_ide.server.is_some() || self.claude_ide.starting.is_some() {
                self.claude_ide_stop();
            }
            self.claude_ide.tried = false;
            return;
        }
        if let Some(starting) = &self.claude_ide.starting {
            match starting.try_recv() {
                Ok(Ok(server)) => {
                    self.claude_ide.starting = None;
                    self.claude_ide_use(server);
                }
                Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => self.claude_ide.starting = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        } else if self.claude_ide.server.is_none() && !self.claude_ide.tried && self.screenshot.is_none() {
            self.claude_ide_start(ctx);
        }
        let Some(server) = &self.claude_ide.server else {
            return;
        };
        server.set_default_style(claude_ide::default_style());
        let events = server.events();
        let connected = server.clients() > 0;
        claude_ide::publish_connected(connected);
        for event in events {
            self.claude_ide_event(event);
        }
        self.claude_ide_reviews();
        let folders = self.claude_folders();
        if folders != self.claude_ide.folders {
            if let Some(server) = &self.claude_ide.server {
                server.set_folders(folders.clone());
            }
            self.claude_ide.folders = folders;
        }
        if connected {
            self.claude_ide_selection(ctx);
        }
    }

    /// Ends the server.
    pub(super) fn claude_ide_stop(&mut self) {
        let tabs: Vec<u64> = self.claude_ide.reviews.drain(..).map(|review| review.tab).collect();
        for tab in tabs {
            self.close_review(tab);
        }
        self.claude_ide.starting = None;
        self.claude_ide.server = None;
        claude_ide::publish_port(None);
        claude_ide::publish_connected(false);
    }

    /// What a client wrote as a file's path, as this computer writes it.
    fn claude_host_path(&self, written: &str, style: PathStyle) -> PathBuf {
        if cfg!(windows) && style == PathStyle::Wsl && written.starts_with('/') {
            return PathBuf::from(claude_ide::from_wsl(written, claude_ide::wsl_distro().as_deref().unwrap_or("")));
        }
        PathBuf::from(written)
    }

    /// A file's path as a client writes it.
    fn claude_client_path(&self, path: &Path, style: PathStyle) -> String {
        let text = path.display().to_string();
        if style == PathStyle::Wsl { claude_ide::to_wsl(&text, claude_ide::wsl_distro().as_deref()) } else { text }
    }

    fn claude_ide_event(&mut self, event: Event) {
        match event {
            Event::Connected => {
                self.claude_ide.selection.again(std::time::Instant::now(), FIRST_SELECTION);
                // Told again as soon as it's time, even if nothing else wakes the window.
                if let Some(ctx) = &self.waker {
                    ctx.request_repaint_after(FIRST_SELECTION);
                }
            }
            Event::Disconnected { client } => {
                let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.claude_ide.reviews).into_iter().partition(|review| review.client == client);
                self.claude_ide.reviews = kept;
                for review in gone {
                    self.close_review(review.tab);
                }
            }
            Event::OpenDiff { client, request, reply } => {
                let style = self.claude_ide.server.as_ref().map_or(PathStyle::Native, |server| server.style(client));
                let path = self.claude_host_path(&request.new_file_path, style);
                // A diff with the same name is replaced, as a new one for the same file would be.
                if let Some(at) = self.claude_ide.reviews.iter().position(|review| review.client == client && review.tab_name == request.tab_name) {
                    let old = self.claude_ide.reviews.remove(at);
                    self.close_review(old.tab);
                }
                match self.open_review(&path, &request.new_file_contents, &request.tab_name) {
                    Ok((tab, decisions)) => self.claude_ide.reviews.push(OpenReview { client, tab_name: request.tab_name, tab, decisions, reply }),
                    // The reply is dropped, which tells the client that it couldn't be done.
                    Err(problem) => self.script_print(problem, false),
                }
            }
            Event::CloseTab { client, tab_name, reply } => {
                if let Some(at) = self.claude_ide.reviews.iter().position(|review| review.client == client && review.tab_name == tab_name) {
                    let review = self.claude_ide.reviews.remove(at);
                    self.close_review(review.tab);
                    // Whoever waited on it is told it was closed.
                    let _ = review.reply.send(claude_ide::diff_answer(&Decision::Closed, &review.tab_name));
                }
                let _ = reply.send(vec!["TAB_CLOSED".to_string()]);
            }
            Event::CloseAllDiffTabs { client, reply } => {
                let (mine, others): (Vec<_>, Vec<_>) = std::mem::take(&mut self.claude_ide.reviews).into_iter().partition(|review| review.client == client);
                self.claude_ide.reviews = others;
                let count = mine.len();
                for review in mine {
                    self.close_review(review.tab);
                    let _ = review.reply.send(claude_ide::diff_answer(&Decision::Closed, &review.tab_name));
                }
                let _ = reply.send(vec![format!("CLOSED_{count}_DIFF_TABS")]);
            }
            Event::OpenFile { client, request, reply } => {
                let style = self.claude_ide.server.as_ref().map_or(PathStyle::Native, |server| server.style(client));
                let path = self.claude_host_path(&request.file_path, style);
                let answer = self.claude_open_file(&path, &request);
                let _ = reply.send(vec![answer]);
            }
            Event::Diagnostics { client, uri, reply } => {
                let style = self.claude_ide.server.as_ref().map_or(PathStyle::Native, |server| server.style(client));
                let json = self.claude_diagnostics(uri.as_deref(), style);
                let _ = reply.send(vec![json.to_string()]);
            }
        }
    }

    /// What `openFile` does: opens the file as a tab, selects what was asked for, and, unless it said not to, brings
    /// the window forward. The answer is what's told to Claude Code.
    fn claude_open_file(&mut self, path: &Path, request: &claude_ide::OpenFileRequest) -> String {
        if !path.is_file() {
            return crate::i18n::tf!("There's no file {}.", path.display());
        }
        if !self.open_path(path) {
            return crate::i18n::tf!("Couldn't open {}.", path.display());
        }
        if let Some(doc) = self.docs.get_mut(self.active).filter(|doc| !doc.hex_mode) {
            let lines: Vec<String> = (0..doc.text.line_count()).map(|i| doc.text.line(i).to_string()).collect();
            if let Some((from, to)) = claude_ide::selection_for(&lines, request.start_text.as_deref(), request.end_text.as_deref(), request.select_to_end_of_line) {
                doc.text.select_chars(from, to);
            }
        }
        if request.make_frontmost
            && let Some(ctx) = &self.waker
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        format!("Opened file: {}", request.file_path)
    }

    /// What `getDiagnostics` answers: the language servers' problems for one file, or every file.
    fn claude_diagnostics(&self, uri: Option<&str>, style: PathStyle) -> serde_json::Value {
        let problems_of = |doc: &Document| doc.text.intel.as_ref().map(|intel| intel.lsp_problems.clone()).unwrap_or_default();
        let mut files = Vec::new();
        match uri {
            Some(uri) => {
                let wanted = claude_ide::path_of_uri(uri).map(|path| self.claude_host_path(&path, style));
                let doc = wanted.as_deref().and_then(|wanted| self.docs.iter().find(|doc| doc.path.as_deref().is_some_and(|path| same_path(path, wanted))));
                // The address comes back as it was asked for, which is how the client checks it's the file it meant.
                files.push((uri.to_string(), doc.map(|doc| doc.text.line_count()), doc.map(problems_of).unwrap_or_default()));
            }
            None => {
                for doc in &self.docs {
                    let (Some(path), problems) = (doc.path.as_deref(), problems_of(doc)) else { continue };
                    if !problems.is_empty() {
                        files.push((claude_ide::file_uri(&self.claude_client_path(path, style)), Some(doc.text.line_count()), problems));
                    }
                }
            }
        }
        claude_ide::diagnostics_json(&files)
    }

    /// Tells Claude Code what the person decided, for each diff decided.
    fn claude_ide_reviews(&mut self) {
        let mut index = 0;
        while index < self.claude_ide.reviews.len() {
            match self.claude_ide.reviews[index].decisions.try_recv() {
                Ok(decision) => {
                    let review = self.claude_ide.reviews.remove(index);
                    let _ = review.reply.send(claude_ide::diff_answer(&decision, &review.tab_name));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => index += 1,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.claude_ide.reviews.remove(index);
                }
            }
        }
    }

    /// The selection in the file in front, for Claude Code, if that's a file with text.
    pub(super) fn claude_current_selection(&self) -> Option<Selection> {
        let doc = self.docs.get(self.active)?;
        if self.no_file() || doc.hex_mode || doc.compare.is_some() || doc.browser.is_some() || doc.table.is_some() || doc.data.is_some() || doc.picture.is_some() {
            return None;
        }
        let path = doc.path.as_deref()?;
        let text = &doc.text;
        let ((start_line, start_col), (end_line, end_col)) = text.selection_char_bounds();
        let utf16 = |line: usize, chars: usize| text.line(line).chars().take(chars).map(char::len_utf16).sum::<usize>();
        let selected = if text.has_selection() && !text.selection_too_large_to_copy() { text.selection_text() } else { String::new() };
        // Copying joins lines with CR LF; the client wants them as the file has them.
        let selected = if matches!(text.terminator(start_line), crate::codec::Eol::CrLf) { selected } else { selected.replace("\r\n", "\n") };
        let selected = if selected.len() > MOST_SELECTED { String::new() } else { selected };
        Some(Selection { path: path.display().to_string(), text: selected, start: (start_line, utf16(start_line, start_col)), end: (end_line, utf16(end_line, end_col)) })
    }

    /// Tells Claude Code of the selection once it's stopped changing.
    fn claude_ide_selection(&mut self, ctx: &Context) {
        let now = std::time::Instant::now();
        let key = self.docs.get(self.active).map(|doc| (doc.id, doc.text.version(), doc.text.caret_line, doc.text.caret_col, doc.text.anchor_line, doc.text.anchor_col));
        if key != self.claude_ide.selection_key {
            self.claude_ide.selection_key = key;
            let current = self.claude_current_selection();
            // With no text file in front there's nothing new to say, so what was last said stands.
            if current.is_some() {
                self.claude_ide.selection.offer(current, now);
            }
        }
        if let Some(selection) = self.claude_ide.selection.ready(now)
            && let Some(server) = &self.claude_ide.server
        {
            let distro = claude_ide::wsl_distro();
            server.notify_all("selection_changed", |style| selection.params(style, distro.as_deref()));
        }
        if let Some(wait) = self.claude_ide.selection.wait(now) {
            ctx.request_repaint_after(wait + Duration::from_millis(5));
        }
    }

    /// Puts the file in front, and the lines selected in it, into Claude Code's prompt as `@file#L3-9`.
    pub(super) fn claude_mention(&mut self) {
        let Some(selection) = self.claude_current_selection() else {
            return self.message("Claude Code", crate::i18n::tr("Open a file with text in it to mention it to Claude Code.").into());
        };
        let Some(server) = &self.claude_ide.server else {
            return;
        };
        let (a, b) = (selection.start, selection.end);
        // A selection that ends at the very start of a line stops at the line before.
        let last = if b.1 == 0 && b.0 > a.0 { b.0 - 1 } else { b.0 };
        let selected = selection.start != selection.end;
        let path = selection.path.clone();
        let distro = claude_ide::wsl_distro();
        server.notify_latest("at_mentioned", |style| {
            let path = if style == PathStyle::Wsl { claude_ide::to_wsl(&path, distro.as_deref()) } else { path.clone() };
            if selected { serde_json::json!({ "filePath": path, "lineStart": a.0, "lineEnd": last }) } else { serde_json::json!({ "filePath": path }) }
        });
    }

    /// What the status bar says of Claude Code, if it's connected: the words, and what they mean.
    pub(super) fn claude_ide_status(&self) -> Option<(String, String)> {
        self.claude_connected().then(|| (crate::i18n::tr("Claude Code connected").to_string(), crate::i18n::tr("Claude Code in the terminal sees what you select here, and its changes to files are shown here for you to accept or reject").to_string()))
    }
}

/// The address Windows gives WSL's network, if there is one: what a `claude` in WSL finds this server by.
pub(crate) fn wsl_gateway() -> Vec<std::net::IpAddr> {
    let Ok(output) = crate::platform::command("ipconfig").output() else {
        return Vec::new();
    };
    claude_ide::wsl_host_address(&String::from_utf8_lossy(&output.stdout)).map(std::net::IpAddr::V4).into_iter().collect()
}
