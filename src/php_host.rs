//! The window's side of the PHP tools (Tools, PHP): each change worked out for the file, the selection or the
//! project and shown first, as Replace in Files shows a replacement, then made in the files open (unsaved, to undo)
//! and on disk for the rest; `php -l` run on each PHP file saved, with its problems marked beside their lines; and
//! F9 on a PHP file running it as a script in the Output panel, or serving its folder with `php -S` and opening the
//! page in the browser.

use super::*;
use crate::php_tools::{self, Change, Finding, Legacy, Piece, Row};
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

/// What's asked for from Tools, PHP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PhpAction {
    LongTags,
    HeadersSent,
    ShortArrays,
    Upgrade,
    Utf8,
    LintNow,
    RunScript,
    Serve,
    StopServing,
}

impl PhpAction {
    /// The tools that can work on a project with nothing open.
    pub(super) fn works_without_file(self) -> bool {
        matches!(self, PhpAction::LongTags | PhpAction::HeadersSent | PhpAction::ShortArrays | PhpAction::Upgrade | PhpAction::Utf8 | PhpAction::StopServing)
    }
}

/// Which of the tools a preview is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tool {
    LongTags,
    Headers,
    Arrays,
    Upgrade,
    Utf8,
}

impl Tool {
    fn title(self) -> &'static str {
        match self {
            Tool::LongTags => "Short tags made long",
            Tool::Headers => "Fix \u{201C}headers already sent\u{201D}",
            Tool::Arrays => "array() to []",
            Tool::Upgrade => "PHP 7 and 8 upgrade check",
            Tool::Utf8 => "Old files made UTF-8",
        }
    }

    fn words(self) -> &'static str {
        match self {
            Tool::LongTags => "<? becomes <?php, which works whatever short_open_tag is set to. <?xml is left as it is.",
            Tool::Headers => "Takes away what PHP sends before a page's headers without being asked: a byte order mark or blank lines before the first <?php, and a closing ?> at the end with anything after it.",
            Tool::Arrays => "array(...) becomes [...], nested ones too, as PHP 5.4 and later allow. Nothing in a string, heredoc or comment is touched.",
            Tool::Upgrade => "What PHP 7 and 8 took away that the code still uses, and what replaces each. The safe ones are fixed for you; the rest are listed to do by hand.",
            Tool::Utf8 => "Files that aren't UTF-8 are read as Windows-1252 (which holds all of Latin-1) and written out as UTF-8.",
        }
    }

    /// Whether it can work on only part of a file.
    fn in_selection(self) -> bool {
        matches!(self, Tool::LongTags | Tool::Arrays | Tool::Upgrade)
    }
}

/// Where a tool works.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scope {
    File,
    Selection,
    Project,
}

/// One file's part in a preview.
pub(super) struct Planned {
    pub path: PathBuf,
    name: String,
    /// The document it's open in, if it is.
    doc_id: Option<u64>,
    /// The bytes the changes were worked out from: the file's, or the open document's (with its byte order mark,
    /// if it has one, at the front), to check nothing's changed before they're made.
    original: Vec<u8>,
    pub changes: Vec<Change>,
    pub findings: Vec<Finding>,
    pub legacy: Option<Legacy>,
    rows: Vec<Row>,
    pub include: bool,
}

pub(super) struct Preview {
    pub tool: Tool,
    pub scope: Scope,
    pub echo_too: bool,
    pub files: Vec<Planned>,
    /// Files that couldn't be looked at, and why.
    pub skipped: Vec<(String, String)>,
    /// What was done by the last Make these changes.
    pub said: String,
}

/// A PHP built-in web server serving a folder.
struct Server {
    job: super::file_tools::RunJob,
    folder: PathBuf,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.job.stop();
    }
}

/// What `php -l` came back with, for a document.
type Lint = (u64, Receiver<Result<Vec<php_tools::LintProblem>, String>>);

#[derive(Default)]
pub(super) struct PhpState {
    pub lint_on_save: bool,
    /// Where PHP is, once looked for.
    found: Option<Option<PathBuf>>,
    linting: Vec<Lint>,
    /// The problems `php -l` marked in each document, to take away when it's checked again.
    marked: Vec<(u64, Vec<(usize, String)>)>,
    pub preview: Option<Preview>,
    server: Option<Server>,
}

impl PhpState {
    pub(super) fn from_settings(settings: &str) -> Self {
        PhpState { lint_on_save: !settings.lines().any(|l| l.trim() == "php_lint_on_save=false"), ..Default::default() }
    }

    pub(super) fn to_settings(&self) -> String {
        if self.lint_on_save { String::new() } else { "php_lint_on_save=false\n".into() }
    }

    pub(super) fn serving(&self) -> bool {
        self.server.is_some()
    }

    /// PHP's program, looked for once: on the PATH, in Homebrew's folders, and where XAMPP, WampServer and
    /// Laragon put it on Windows.
    fn php(&mut self) -> Option<PathBuf> {
        self.found.get_or_insert_with(|| crate::emulators::find_program(&["php"], &["php", "xampp", "wamp", "wamp64", "laragon"])).clone()
    }
}

/// What to say when PHP can't be found.
fn not_found() -> String {
    crate::i18n::tr("PHP isn't installed, or isn't where Limoniter can find it. It's at php.net (windows.php.net for Windows); on a Mac, brew install php; on Debian or Ubuntu, sudo apt install php-cli. Then put it on the PATH.").into()
}

/// A path or program as the shell wants it, in quotes.
fn quoted(text: &str) -> String {
    if cfg!(windows) { format!("\"{text}\"") } else { format!("'{}'", text.replace('\'', "'\\''")) }
}

/// A file's address on the server, from the folder it serves.
fn address_of(port: u16, folder: &Path, file: Option<&Path>) -> String {
    let relative = file.and_then(|f| f.strip_prefix(folder).ok()).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
    let encoded: String = relative.chars().map(|c| match c {
        ' ' => "%20".to_string(),
        '#' => "%23".to_string(),
        '?' => "%3F".to_string(),
        '%' => "%25".to_string(),
        c => c.to_string(),
    }).collect();
    format!("http://localhost:{port}/{encoded}")
}

/// The first port from 8000 nothing's listening on.
fn free_port() -> Option<u16> {
    (8000..8100).find(|port| std::net::TcpListener::bind(("127.0.0.1", *port)).is_ok())
}

/// A byte offset in text held as lines joined by \n, from a line and a character along it.
fn byte_offset(lines: &[Arc<str>], line: usize, char_index: usize) -> usize {
    let before: usize = lines.iter().take(line).map(|l| l.len() + 1).sum();
    let along = lines.get(line).map_or(0, |l| l.char_indices().nth(char_index).map_or(l.len(), |(at, _)| at));
    before + along
}

impl Limoniter {
    /// Whether the document is PHP, by its name or the language it's coloured as.
    pub(super) fn is_php_doc(&self, index: usize) -> bool {
        let doc = &self.docs[index];
        !doc.hex_mode && doc.browser.is_none() && (doc.path.as_deref().is_some_and(php_tools::is_php_file) || doc.language_name() == "PHP")
    }

    /// Whether the PHP menu's command can be chosen now.
    pub(super) fn php_can(&self, action: PhpAction) -> bool {
        let text = !self.no_file() && !self.docs[self.active].hex_mode;
        let project = self.project.as_ref().is_some_and(|p| !p.files.is_empty());
        match action {
            PhpAction::StopServing => self.php.serving(),
            PhpAction::LintNow | PhpAction::RunScript | PhpAction::Serve => text,
            _ => text || project,
        }
    }

    pub(super) fn php_command(&mut self, action: PhpAction) {
        let tool = match action {
            PhpAction::LongTags => Tool::LongTags,
            PhpAction::HeadersSent => Tool::Headers,
            PhpAction::ShortArrays => Tool::Arrays,
            PhpAction::Upgrade => Tool::Upgrade,
            PhpAction::Utf8 => Tool::Utf8,
            PhpAction::LintNow => return self.php_lint_now(),
            PhpAction::RunScript => return self.php_run(false),
            PhpAction::Serve => return self.php_run(true),
            PhpAction::StopServing => return self.php_stop_serving(),
        };
        // The selection, if there is one and the tool works on part of a file; the file in front if it's PHP;
        // otherwise the whole project.
        let front = (!self.no_file()).then_some(self.active).filter(|i| !self.docs[*i].hex_mode);
        let scope = match front {
            Some(i) if tool.in_selection() && self.docs[i].text.has_selection() => Scope::Selection,
            Some(i) if self.is_php_doc(i) => Scope::File,
            _ if self.project.is_some() || front.is_some_and(|i| self.docs[i].path.is_some()) => Scope::Project,
            _ => Scope::File,
        };
        let scope = if tool == Tool::Utf8 && front.is_some_and(|i| self.docs[i].text.format.charset.is_unicode()) && scope == Scope::File { Scope::Project } else { scope };
        self.php.preview = Some(self.php_plan(tool, scope, false));
    }

    /// The PHP files the project scope covers, and the name it goes by: the project's, or else those in the
    /// folder of the file in front and the folders in it.
    fn php_project_files(&self) -> Option<(String, Vec<PathBuf>)> {
        if let Some(project) = self.project.as_ref().filter(|p| !p.files.is_empty()) {
            let name = project.root.as_deref().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| project.name.clone());
            return Some((name, project.files.iter().filter(|p| php_tools::is_php_file(p)).cloned().collect()));
        }
        let folder = self.docs.get(self.active).filter(|_| !self.no_file()).and_then(|d| d.path.as_deref()).and_then(Path::parent)?;
        let name = folder.file_name().map_or_else(|| folder.display().to_string(), |n| n.to_string_lossy().into_owned());
        Some((name, php_tools::php_files_in(folder, 5000)))
    }

    /// An open document's text as bytes, joined by \n, with its byte order mark at the front if it has one.
    fn php_doc_bytes(&self, index: usize) -> Vec<u8> {
        let doc = &self.docs[index];
        let mut bytes = if doc.text.format.has_bom { b"\xEF\xBB\xBF".to_vec() } else { Vec::new() };
        bytes.extend_from_slice(doc.text.all_lines().iter().map(|l| &**l).collect::<Vec<_>>().join("\n").as_bytes());
        bytes
    }

    /// Why an open document can't be changed by a tool, if it can't.
    fn php_doc_unchangeable(&self, index: usize) -> Option<String> {
        let doc = &self.docs[index];
        if doc.hex_mode {
            Some(crate::i18n::tr("it's open in hex mode").into())
        } else if doc.text.is_disk_backed() {
            Some(crate::i18n::tr("it's open, and too large to change in its window").into())
        } else if doc.text.read_only {
            Some(crate::i18n::tr("it's open and set to read-only").into())
        } else if doc.is_busy() {
            Some(crate::i18n::tr("it's open and busy").into())
        } else {
            None
        }
    }

    /// Works out what a tool would change, file by file, changing nothing.
    pub(super) fn php_plan(&self, tool: Tool, scope: Scope, echo_too: bool) -> Preview {
        let mut preview = Preview { tool, scope, echo_too, files: Vec::new(), skipped: Vec::new(), said: String::new() };
        // Each file: its path, its document, and its bytes.
        let mut sources: Vec<(PathBuf, String, Option<u64>, Vec<u8>)> = Vec::new();
        let mut range = None;
        match scope {
            Scope::File | Scope::Selection => {
                if self.no_file() {
                    return preview;
                }
                let index = self.active;
                let doc = &self.docs[index];
                let path = doc.path.clone().unwrap_or_else(|| PathBuf::from(&doc.untitled));
                if let Some(why) = self.php_doc_unchangeable(index) {
                    preview.skipped.push((doc.name(), why));
                    return preview;
                }
                if scope == Scope::Selection {
                    let ((l0, c0), (l1, c1)) = doc.text.selection_char_bounds();
                    let ((l0, c0), (l1, c1)) = if (l0, c0) <= (l1, c1) { ((l0, c0), (l1, c1)) } else { ((l1, c1), (l0, c0)) };
                    let lines = doc.text.all_lines();
                    let bom = if doc.text.format.has_bom { 3 } else { 0 };
                    range = Some(bom + byte_offset(&lines, l0, c0)..bom + byte_offset(&lines, l1, c1));
                }
                sources.push((path, doc.name(), Some(doc.id), self.php_doc_bytes(index)));
            }
            Scope::Project => {
                let Some((_, files)) = self.php_project_files() else {
                    return preview;
                };
                for path in files {
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    if let Some(index) = self.doc_for_path(&path) {
                        if let Some(why) = self.php_doc_unchangeable(index) {
                            preview.skipped.push((name, why));
                        } else {
                            sources.push((path, name, Some(self.docs[index].id), self.php_doc_bytes(index)));
                        }
                        continue;
                    }
                    match std::fs::read(&path) {
                        Ok(bytes) if bytes.len() > 16 * 1024 * 1024 => preview.skipped.push((name, crate::i18n::tr("it's too large").into())),
                        // UTF-16 and UTF-32 have a nought in nearly every character; PHP can't read them anyway.
                        Ok(bytes) if bytes.contains(&0) => preview.skipped.push((name, crate::i18n::tr("it looks like a binary file").into())),
                        Ok(bytes) => sources.push((path, name, None, bytes)),
                        Err(e) => preview.skipped.push((name, e.to_string())),
                    }
                }
            }
        }
        for (path, name, doc_id, bytes) in sources {
            let mut planned = Planned { path, name, doc_id, original: Vec::new(), changes: Vec::new(), findings: Vec::new(), legacy: None, rows: Vec::new(), include: true };
            match tool {
                Tool::LongTags => planned.changes = php_tools::long_tags(&bytes, echo_too),
                Tool::Headers => planned.changes = php_tools::headers_fixes(&bytes),
                Tool::Arrays => planned.changes = php_tools::short_arrays(&bytes),
                Tool::Upgrade => {
                    planned.findings = php_tools::upgrade_findings(&bytes);
                    if let Some(range) = &range {
                        planned.findings.retain(|f| f.start >= range.start && f.end <= range.end);
                    }
                    planned.changes = planned.findings.iter().filter_map(|f| f.fix.clone()).collect();
                }
                Tool::Utf8 => {
                    // An open document's characters are already read; it's the bytes it's saved as that count.
                    // UTF-8 with bytes it couldn't read kept as they were counts, as well as the two old sets.
                    let stored = match doc_id.and_then(|id| self.docs.iter().find(|d| d.id == id)) {
                        Some(doc) if doc.text.format.charset.is_unicode() || matches!(doc.text.format.charset.id.as_ref(), "windows-1252" | "iso-8859-1" | "iso-8859-15") => doc.text.encode(&doc.text.format, codec::Lossy::Refuse).ok(),
                        Some(doc) => {
                            preview.skipped.push((planned.name.clone(), crate::i18n::tf!("it's open as {}, not Latin-1 or Windows-1252", doc.text.format.encoding_name())));
                            None
                        }
                        None => Some(bytes.clone()),
                    };
                    planned.legacy = stored.as_deref().and_then(php_tools::legacy_text);
                }
            }
            if let Some(range) = &range {
                planned.changes.retain(|c| c.within(range));
            }
            planned.rows = php_tools::preview_rows(&bytes, &planned.changes);
            planned.original = bytes;
            if !planned.changes.is_empty() || !planned.findings.is_empty() || planned.legacy.is_some() {
                preview.files.push(planned);
            }
        }
        preview
    }

    /// Makes the changes in the preview's files that are ticked, and says what was done.
    fn php_make_changes(&mut self, preview: &Preview) -> String {
        let mut open_changed = 0;
        let mut disk_changed = 0;
        let mut backups = 0;
        let mut skipped: Vec<(String, String)> = Vec::new();
        let backup = self.find.keep_backups;
        for planned in preview.files.iter().filter(|p| p.include) {
            if planned.changes.is_empty() && planned.legacy.is_none() {
                continue;
            }
            if let Some(index) = planned.doc_id.and_then(|id| self.docs.iter().position(|d| d.id == id)) {
                if self.php_doc_bytes(index) != planned.original {
                    skipped.push((planned.name.clone(), crate::i18n::tr("it changed after the preview").into()));
                    continue;
                }
                if let Some(why) = self.php_doc_unchangeable(index) {
                    skipped.push((planned.name.clone(), why));
                    continue;
                }
                if let Some(legacy) = &planned.legacy {
                    let format = &self.docs[index].text.format;
                    if format.charset.is_unicode() {
                        // Read as UTF-8, with the bytes that weren't kept as they were: those are read again.
                        let rows: Vec<String> = legacy.text.split('\n').map(|l| l.trim_end_matches('\r').to_string()).collect();
                        let doc = &mut self.docs[index];
                        let caret = doc.text.caret_char_position();
                        let last = doc.text.line_count().saturating_sub(1);
                        doc.text.replace_lines(0, last, rows);
                        doc.text.move_caret_to_char(caret.0.min(doc.text.line_count().saturating_sub(1)), caret.1);
                    } else {
                        let target = TextFormat { charset: crate::charset::utf8(), has_bom: false, ..format.clone() };
                        self.convert_to(index, target);
                    }
                    open_changed += 1;
                    continue;
                }
                let changed = php_tools::apply(&planned.original, &planned.changes);
                let (bom, body) = match changed.strip_prefix(b"\xEF\xBB\xBF") {
                    Some(body) => (true, body),
                    None => (false, &changed[..]),
                };
                let Ok(text) = std::str::from_utf8(body) else {
                    skipped.push((planned.name.clone(), crate::i18n::tr("something went wrong reading it").into()));
                    continue;
                };
                let rows: Vec<String> = text.split('\n').map(str::to_string).collect();
                let doc = &mut self.docs[index];
                let caret = doc.text.caret_char_position();
                let last = doc.text.line_count().saturating_sub(1);
                doc.text.replace_lines(0, last, rows);
                if doc.text.format.has_bom != bom {
                    let format = TextFormat { has_bom: bom, ..doc.text.format.clone() };
                    doc.text.change_format(format);
                }
                let line = caret.0.min(doc.text.line_count().saturating_sub(1));
                doc.text.move_caret_to_char(line, caret.1);
                open_changed += 1;
                continue;
            }
            match std::fs::read(&planned.path) {
                Ok(now) if now == planned.original => {}
                Ok(_) => {
                    skipped.push((planned.name.clone(), crate::i18n::tr("it changed after the preview").into()));
                    continue;
                }
                Err(e) => {
                    skipped.push((planned.name.clone(), e.to_string()));
                    continue;
                }
            }
            let changed = match &planned.legacy {
                Some(legacy) => legacy.text.clone().into_bytes(),
                None => php_tools::apply(&planned.original, &planned.changes),
            };
            if backup {
                if let Err(e) = codec::write_file(&crate::find_files::free_backup_name(&planned.path), &planned.original) {
                    skipped.push((planned.name.clone(), crate::i18n::tf!("its backup couldn't be written: {e}")));
                    continue;
                }
                backups += 1;
            }
            match codec::write_file(&planned.path, &changed) {
                Ok(()) => disk_changed += 1,
                Err(e) => skipped.push((planned.name.clone(), e.to_string())),
            }
        }
        let mut said = match (open_changed, disk_changed) {
            (0, 0) => crate::i18n::tr("Nothing was changed.").to_string(),
            (open, 0) => crate::i18n::tf!("Changed {open} open file(s), unsaved: Undo takes it back."),
            (0, disk) => crate::i18n::tf!("Changed {disk} file(s) on disk."),
            (open, disk) => crate::i18n::tf!("Changed {open} open file(s), unsaved, and {disk} on disk."),
        };
        if backups > 0 {
            said += " ";
            said += &crate::i18n::tf!("Each one on disk has a .bak copy of how it was.");
        }
        for (name, why) in skipped {
            said += "\n";
            said += &crate::i18n::tf!("{name} was left: {why}.");
        }
        said
    }

    /// Goes to a line of a file in a preview.
    fn php_go_to(&mut self, path: &Path, doc_id: Option<u64>, line: usize) {
        let index = match doc_id.and_then(|id| self.docs.iter().position(|d| d.id == id)) {
            Some(index) => index,
            None => {
                if !self.open_path(path) {
                    return;
                }
                self.active
            }
        };
        self.active = index;
        self.pending_focus = true;
        let doc = &mut self.docs[index];
        if !doc.hex_mode && line < doc.text.line_count() {
            doc.text.move_caret(line, 0, false);
        }
    }

    /// The preview window: what a tool will change, file by file, and the buttons to go ahead.
    pub(super) fn php_window(&mut self, ctx: &Context) {
        let Some(mut preview) = self.php.preview.take() else { return };
        let mut open = true;
        let mut close = false;
        let mut go = false;
        let mut again: Option<(Scope, bool)> = None;
        let mut jump: Option<(PathBuf, Option<u64>, usize)> = None;
        let has_selection = !self.no_file() && !self.docs[self.active].hex_mode && self.docs[self.active].text.has_selection();
        let project_name = self.php_project_files().map(|(name, _)| name);
        let file_name = (!self.no_file()).then(|| self.docs[self.active].name());
        let backup = self.find.keep_backups;
        crate::fit::Dialog::new(crate::i18n::tr(preview.tool.title())).open(&mut open).collapsible(false).resizable(true).default_width(640.0).show(ctx, |ui| {
            ui.label(RichText::new(crate::i18n::tr(preview.tool.words())).color(theme::text_dim()));
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                let mut scope = preview.scope;
                if let Some(name) = &file_name {
                    ui.selectable_value(&mut scope, Scope::File, crate::i18n::tf!("This file ({name})"));
                }
                if preview.tool.in_selection() && has_selection {
                    ui.selectable_value(&mut scope, Scope::Selection, crate::i18n::tr("The selection"));
                }
                if let Some(name) = &project_name {
                    ui.selectable_value(&mut scope, Scope::Project, crate::i18n::tf!("Every PHP file in {name}"));
                }
                let mut echo_too = preview.echo_too;
                if preview.tool == Tool::LongTags {
                    ui.separator();
                    ui.checkbox(&mut echo_too, crate::i18n::tr("<?= to <?php echo too")).on_hover_text(crate::i18n::tr("<?= works whatever the settings, so it's usually left as it is"));
                }
                if scope != preview.scope || echo_too != preview.echo_too {
                    again = Some((scope, echo_too));
                }
            });
            ui.separator();
            if !preview.said.is_empty() {
                ui.label(RichText::new(&preview.said).color(theme::accent()));
                ui.add_space(4.0);
            }
            let ticked: Vec<&Planned> = preview.files.iter().filter(|p| p.include).collect();
            let changes: usize = ticked.iter().map(|p| p.changes.len() + usize::from(p.legacy.is_some())).sum();
            let summary = match preview.tool {
                Tool::Upgrade => {
                    let found: usize = preview.files.iter().map(|p| p.findings.len()).sum();
                    if found == 0 { crate::i18n::tr("Nothing that newer PHP took away was found.").to_string() } else { crate::i18n::tf!("{found} thing(s) found in {} file(s); {changes} of them can be fixed for you.", preview.files.len()) }
                }
                Tool::Utf8 if preview.files.is_empty() => crate::i18n::tr("Every file is UTF-8 already (or plain ASCII).").into(),
                Tool::Utf8 => crate::i18n::tf!("{} file(s) to make UTF-8.", preview.files.len()),
                _ if preview.files.is_empty() => crate::i18n::tr("Nothing to change.").into(),
                _ => crate::i18n::tf!("{changes} change(s) in {} file(s).", ticked.len()),
            };
            ui.label(RichText::new(summary).strong());
            let font = egui::FontId::monospace(12.5);
            egui::ScrollArea::vertical().max_height(420.0).auto_shrink([false, true]).show(ui, |ui| {
                for planned in &mut preview.files {
                    ui.horizontal(|ui| {
                        let count = planned.changes.len().max(planned.findings.len());
                        let label = if planned.legacy.is_some() { planned.name.clone() } else { format!("{}  ({count})", planned.name) };
                        if preview.tool == Tool::Upgrade && planned.changes.is_empty() {
                            ui.label(RichText::new(label).strong());
                        } else {
                            ui.checkbox(&mut planned.include, RichText::new(label).strong());
                        }
                        if let Some(folder) = planned.path.parent() {
                            ui.label(RichText::new(folder.display().to_string()).small().color(theme::text_faint()));
                        }
                    });
                    ui.indent(("php-file", &planned.path), |ui| {
                        if let Some(legacy) = &planned.legacy {
                            ui.label(RichText::new(if legacy.windows { crate::i18n::tr("Read as Windows-1252") } else { crate::i18n::tr("Read as Latin-1 (ISO-8859-1)") }).color(theme::text_dim()));
                            for (line, text) in &legacy.samples {
                                if ui.add(egui::Label::new(RichText::new(format!("{}: {text}", line + 1)).font(font.clone())).truncate().sense(egui::Sense::click())).on_hover_text(crate::i18n::tr("Go to this line")).clicked() {
                                    jump = Some((planned.path.clone(), planned.doc_id, *line));
                                }
                            }
                            for (line, text) in &legacy.declared {
                                ui.label(RichText::new(crate::i18n::tf!("Line {} still says the old encoding, to change by hand: {text}", line + 1)).color(theme::mistake()));
                            }
                        }
                        if preview.tool == Tool::Upgrade {
                            for finding in &planned.findings {
                                let line = php_tools::line_at(&planned.original, finding.start);
                                let fixed = if finding.fix.is_some() { crate::i18n::tr("fixed for you") } else { crate::i18n::tr("to do by hand") };
                                let words = crate::i18n::tf!("{}: {}, gone in PHP {}. Instead: {}. ({fixed})", line + 1, finding.found, finding.removed_in, crate::i18n::tr(finding.instead.words()));
                                let colour = if finding.fix.is_some() { theme::text() } else { theme::mistake() };
                                if ui.add(egui::Label::new(RichText::new(words).color(colour)).wrap().sense(egui::Sense::click())).on_hover_text(crate::i18n::tr("Go to this line")).clicked() {
                                    jump = Some((planned.path.clone(), planned.doc_id, line));
                                }
                            }
                            if !planned.rows.is_empty() {
                                ui.label(RichText::new(crate::i18n::tr("The fixes:")).color(theme::text_dim()));
                            }
                        }
                        for row in planned.rows.iter().take(300) {
                            let mut job = egui::text::LayoutJob::default();
                            job.append(&format!("{:>5}  ", row.line + 1), 0.0, egui::TextFormat { font_id: font.clone(), color: theme::text_faint(), ..Default::default() });
                            for piece in &row.pieces {
                                let (text, format) = match piece {
                                    Piece::Same(text) => (text, egui::TextFormat { font_id: font.clone(), color: theme::text_dim(), ..Default::default() }),
                                    Piece::Gone(text) => (text, egui::TextFormat { font_id: font.clone(), color: theme::text(), background: theme::compare_removed(), strikethrough: egui::Stroke::new(1.0, theme::text()), ..Default::default() }),
                                    Piece::Added(text) => (text, egui::TextFormat { font_id: font.clone(), color: theme::text(), background: theme::compare_added(), ..Default::default() }),
                                };
                                let text: String = if text.chars().count() > 160 { text.chars().take(159).chain(['\u{2026}']).collect() } else { text.clone() };
                                job.append(&text, 0.0, format);
                            }
                            job.wrap.max_rows = 1;
                            job.wrap.max_width = ui.available_width();
                            if ui.add(egui::Label::new(job).sense(egui::Sense::click())).on_hover_text(crate::i18n::tr("Go to this line")).clicked() {
                                jump = Some((planned.path.clone(), planned.doc_id, row.line));
                            }
                        }
                        if planned.rows.len() > 300 {
                            ui.label(RichText::new(crate::i18n::tf!("and {} more", planned.rows.len() - 300)).color(theme::text_faint()));
                        }
                    });
                }
                if !preview.skipped.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(crate::i18n::tr("Left out:")).color(theme::text_dim()));
                    for (name, why) in &preview.skipped {
                        ui.label(RichText::new(crate::i18n::tf!("{name}: {why}")).color(theme::text_faint()));
                    }
                }
            });
            ui.separator();
            if changes > 0 {
                ui.label(crate::i18n::tr("Files open here change in their windows, unsaved, and can be undone."));
                ui.label(if backup { crate::i18n::tr("The others are rewritten on disk now, each with a .bak copy of how it was.") } else { crate::i18n::tr("The others are rewritten on disk now, and that can't be undone.") });
            }
            ui.horizontal(|ui| {
                let label = if preview.tool == Tool::Upgrade { crate::i18n::tr("Fix the safe ones") } else { crate::i18n::tr("Make these changes") };
                go = ui.add_enabled(changes > 0, egui::Button::new(label)).clicked();
                close = ui.button(crate::i18n::tr("Close")).clicked();
            });
        });
        if let Some((path, doc_id, line)) = jump {
            self.php_go_to(&path, doc_id, line);
        }
        if !open || close {
            return;
        }
        if go {
            let said = self.php_make_changes(&preview);
            // Looked at again, to show what's left.
            preview = Preview { said, ..self.php_plan(preview.tool, preview.scope, preview.echo_too) };
        } else if let Some((scope, echo_too)) = again {
            preview = self.php_plan(preview.tool, scope, echo_too);
        }
        self.php.preview = Some(preview);
    }

    // ---------- php -l ----------

    /// Starts `php -l` on a document: on its file, as saved, or else on its text, given to PHP to read.
    fn php_lint(&mut self, index: usize, file: Option<PathBuf>) -> Result<(), String> {
        let php = self.php.php().ok_or_else(not_found)?;
        let id = self.docs[index].id;
        let text = if file.is_none() { Some(self.document_bytes_of(index)) } else { None };
        let (send, receive) = channel();
        let wake = self.waker.clone();
        std::thread::spawn(move || {
            let _ = send.send(lint_with(&php, file.as_deref(), text.as_deref()));
            if let Some(ctx) = wake {
                ctx.request_repaint();
            }
        });
        self.php.linting.retain(|(doc, _)| *doc != id);
        self.php.linting.push((id, receive));
        Ok(())
    }

    /// The document's text as it would be saved.
    fn document_bytes_of(&self, index: usize) -> Vec<u8> {
        let doc = &self.docs[index];
        doc.text.encode(&doc.text.format, codec::Lossy::Refuse).unwrap_or_else(|_| doc.text.all_lines().iter().map(|l| &**l).collect::<Vec<_>>().join("\n").into_bytes())
    }

    /// After a save: a PHP file is checked by `php -l`, if that's on and PHP is installed.
    pub(super) fn php_saved(&mut self, index: usize, path: &Path) {
        if !self.php.lint_on_save || !self.is_php_doc(index) || self.php.php().is_none() {
            return;
        }
        let _ = self.php_lint(index, Some(path.to_path_buf()));
    }

    fn php_lint_now(&mut self) {
        if self.no_file() {
            return;
        }
        let index = self.active;
        // The file as saved, if it is; otherwise the text as it is now.
        let file = self.docs[index].path.clone().filter(|_| !self.docs[index].is_modified());
        if let Err(problem) = self.php_lint(index, file) {
            self.message("Check with php -l", problem);
        }
    }

    /// Brings back what `php -l` said, marking its problems beside their lines. True while any is still running.
    pub(super) fn php_tick(&mut self) -> bool {
        let mut done = Vec::new();
        self.php.linting.retain(|(id, receive)| match receive.try_recv() {
            Ok(result) => {
                done.push((*id, result));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        for (id, result) in done {
            self.php_lint_done(id, result);
        }
        let serving = self.php_server_tick();
        !self.php.linting.is_empty() || serving
    }

    fn php_lint_done(&mut self, id: u64, result: Result<Vec<php_tools::LintProblem>, String>) {
        let Some(index) = self.docs.iter().position(|d| d.id == id) else { return };
        let problems = match result {
            Ok(problems) => problems,
            Err(problem) => {
                self.running.note = Some((problem, Instant::now()));
                return;
            }
        };
        // What was marked last time goes, and the new problems take its place.
        let before = self.php.marked.iter().position(|(d, _)| *d == id).map(|at| self.php.marked.remove(at).1).unwrap_or_default();
        let doc = &mut self.docs[index];
        if let Some(intel) = doc.text.intel.as_mut() {
            intel.build_problems.retain(|p| !before.contains(p));
        }
        let name = doc.name();
        let shown = doc.path.as_deref().map_or_else(|| name.clone(), |p| p.display().to_string());
        if problems.is_empty() {
            if !before.is_empty() {
                self.running.note = Some((crate::i18n::tf!("{name}: no syntax errors now."), Instant::now()));
            }
            return;
        }
        let marks: Vec<(usize, String)> = problems.iter().map(|p| (p.line.saturating_sub(1), p.message.clone())).collect();
        let intel = doc.text.intel.get_or_insert_with(Box::default);
        for mark in &marks {
            if !intel.build_problems.contains(mark) {
                intel.build_problems.push(mark.clone());
            }
        }
        self.php.marked.push((id, marks));
        for problem in &problems {
            // As compilers print a problem, so clicking it in the Output panel goes there.
            self.script_print(format!("{shown}:{}: {}", problem.line, problem.message), false);
        }
        let first = problems[0].line;
        self.running.note = Some((crate::i18n::tf!("PHP found a problem in {name}, on line {first}."), Instant::now()));
    }

    // ---------- Running and serving ----------

    /// F9 on a PHP file: run it, or serve it if it's a page. False if the file in front isn't PHP.
    pub(super) fn php_run_on_f9(&mut self) -> bool {
        if self.no_file() || !self.is_php_doc(self.active) {
            return false;
        }
        let page = php_tools::is_web_page(&self.php_doc_bytes(self.active));
        self.php_run(page);
        true
    }

    /// Runs the file in front as a script, with what it prints in the Output panel; or, as a page, serves its
    /// folder with PHP's own web server and opens it in the browser.
    fn php_run(&mut self, serve: bool) {
        let Some(php) = self.php.php() else {
            return self.message("Run PHP", not_found());
        };
        let index = self.active;
        if self.docs[index].path.is_none() && !self.save_as(index) {
            return;
        }
        let Some(path) = self.docs[index].path.clone() else { return };
        if !serve {
            let line = format!("{} {}", quoted(&php.display().to_string()), quoted(&path.display().to_string()));
            if let Err(problem) = self.run_tool_in(&line, true, path.parent().map(Path::to_path_buf)) {
                self.message("Run PHP", problem);
            }
            return;
        }
        if self.docs[index].is_modified() && !self.save(index) {
            return;
        }
        // The project's folder, if the file is in it, or else the file's own, as the live server serves.
        let folder = self.project.as_ref().map(|p| p.folder()).filter(|f| path.starts_with(f)).or_else(|| path.parent().map(Path::to_path_buf)).unwrap_or_default();
        if let Some(server) = self.php.server.as_ref().filter(|s| same_path(&s.folder, &folder)) {
            let _ = crate::platform::open_web_page(&address_of(server.port, &folder, Some(&path)));
            return;
        }
        self.php.server = None;
        let Some(port) = free_port() else {
            return self.message("Serve with PHP", crate::i18n::tr("No port from 8000 to 8099 is free to serve on.").into());
        };
        let line = format!("{} -S localhost:{port} -t {}", quoted(&php.display().to_string()), quoted(&folder.display().to_string()));
        let wake = self.waker.clone();
        match super::file_tools::start_run(&line, Some(&folder), move || {
            if let Some(ctx) = &wake {
                ctx.request_repaint();
            }
        }) {
            Ok(job) => {
                self.script_print(crate::i18n::tf!("> {line}"), false);
                self.script_print(crate::i18n::tf!("Serving {} at http://localhost:{port}/ on this computer only. Tools, PHP, Stop serving ends it.", folder.display()), false);
                let address = address_of(port, &folder, Some(&path));
                // The browser's sent there once the server's listening, which takes it a moment.
                std::thread::spawn(move || {
                    let began = Instant::now();
                    while began.elapsed() < Duration::from_secs(5) && std::net::TcpStream::connect(("localhost", port)).is_err() {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    let _ = crate::platform::open_web_page(&address);
                });
                self.php.server = Some(Server { job, folder, port });
            }
            Err(problem) => self.message("Serve with PHP", problem),
        }
    }

    pub(super) fn php_stop_serving(&mut self) {
        if let Some(server) = self.php.server.take() {
            let folder = server.folder.display().to_string();
            drop(server);
            self.script_print(crate::i18n::tf!("{folder} isn't being served any more."), false);
        }
    }

    /// What the server has printed, into the Output panel. True while it's serving.
    fn php_server_tick(&mut self) -> bool {
        let Some(server) = self.php.server.as_mut() else { return false };
        let (lines, finished) = server.job.take_output();
        for line in lines {
            self.script_print(line, false);
        }
        if let Some(summary) = finished {
            self.script_print(crate::i18n::tf!("PHP's server stopped. {summary}"), false);
            self.php.server = None;
            return false;
        }
        true
    }
}

/// Runs `php -l` and reads what it says. Without php.ini (-n), so nothing else it would load gets in the way; its
/// errors on the screen and not in a log, so they're printed once.
fn lint_with(php: &Path, file: Option<&Path>, text: Option<&[u8]>) -> Result<Vec<php_tools::LintProblem>, String> {
    use std::io::Write;
    let mut command = crate::platform::command(php);
    command.args(["-n", "-d", "display_errors=1", "-d", "log_errors=0", "-l"]);
    if let Some(file) = file {
        command.arg(file);
    }
    command.stdin(if text.is_some() { std::process::Stdio::piped() } else { std::process::Stdio::null() }).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = command.spawn().map_err(|e| crate::i18n::tf!("PHP couldn't be run: {e}"))?;
    if let (Some(text), Some(mut stdin)) = (text, child.stdin.take()) {
        let _ = stdin.write_all(text);
    }
    let output = child.wait_with_output().map_err(|e| crate::i18n::tf!("PHP couldn't be run: {e}"))?;
    let printed = format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    Ok(php_tools::lint_problems(&printed))
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-php-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The project's files are looked at, nothing changes until it's asked for, and then open files change in
    /// their tabs (to undo) and the rest on disk, exactly as they were but for the change.
    #[test]
    fn a_project_s_files_change_only_after_the_preview() {
        let (mut app, _ctx) = app_with("");
        let dir = folder("project");
        std::fs::write(dir.join("a.php"), "<?php\n$a = array(1);\n").unwrap();
        std::fs::write(dir.join("b.php"), b"\xEF\xBB\xBF<?php\r\n$b = array('x' => \"array(\", 2);\r\n").unwrap();
        std::fs::write(dir.join("c.php"), "<?php\n$c = [3];\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "array(4)\n").unwrap();
        assert!(app.open_path(&dir.join("a.php")));
        let preview = app.php_plan(Tool::Arrays, Scope::Project, false);
        let names: Vec<&str> = preview.files.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["a.php", "b.php"], "c.php has nothing to change, and notes.txt isn't PHP");
        assert_eq!(app.doc().text.all_lines()[1].as_ref(), "$a = array(1);", "the preview changes nothing");
        let said = app.php_make_changes(&preview);
        assert!(said.contains('1'), "{said}");
        assert_eq!(app.doc().text.all_lines()[1].as_ref(), "$a = [1];");
        assert!(app.doc().is_modified(), "an open file is changed in its tab, unsaved");
        assert_eq!(std::fs::read_to_string(dir.join("a.php")).unwrap(), "<?php\n$a = array(1);\n");
        assert_eq!(std::fs::read(dir.join("b.php")).unwrap(), b"\xEF\xBB\xBF<?php\r\n$b = ['x' => \"array(\", 2];\r\n", "its mark, its line endings and its string are kept");
        app.doc().text.undo();
        assert_eq!(app.doc().text.all_lines()[1].as_ref(), "$a = array(1);", "one Undo takes it back");
        // A file changed after its preview is left as it is.
        std::fs::write(dir.join("b.php"), "<?php $b = array(6);").unwrap();
        let preview = app.php_plan(Tool::Arrays, Scope::Project, false);
        std::fs::write(dir.join("b.php"), "<?php $b = array(5);").unwrap();
        let said = app.php_make_changes(&preview);
        assert!(said.contains("b.php"), "{said}");
        assert_eq!(std::fs::read_to_string(dir.join("b.php")).unwrap(), "<?php $b = array(5);");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// In the selection, only what's in it; the headers fix takes an open file's byte order mark off; the upgrade
    /// check fixes the safe ones and lists the rest.
    #[test]
    fn the_tools_work_on_the_file_and_the_selection() {
        let (mut app, _ctx) = app_with("");
        let dir = folder("file");
        let path = dir.join("page.php");
        std::fs::write(&path, b"\xEF\xBB\xBF\n<? $a = array(1);\n$b = array(2);\n$c = $s{0};\n$r = mysql_query($q);\n?>\n\n").unwrap();
        assert!(app.open_path(&path));
        assert!(app.doc().text.format.has_bom);
        app.doc().text.select_chars((2, 0), (2, 14));
        app.php_command(PhpAction::ShortArrays);
        let preview = app.php.preview.take().unwrap();
        assert_eq!(preview.scope, Scope::Selection);
        app.php_make_changes(&preview);
        assert_eq!(app.doc().text.all_lines()[1..3].iter().map(|l| l.to_string()).collect::<Vec<_>>(), ["<? $a = array(1);", "$b = [2];"]);
        let preview = app.php_plan(Tool::LongTags, Scope::File, false);
        app.php_make_changes(&preview);
        assert_eq!(app.doc().text.all_lines()[1].as_ref(), "<?php $a = array(1);");
        let preview = app.php_plan(Tool::Upgrade, Scope::File, false);
        let found: Vec<(&str, bool)> = preview.files[0].findings.iter().map(|f| (f.found.as_str(), f.fix.is_some())).collect();
        assert_eq!(found, [("$s{0}", true), ("mysql_query", false)]);
        app.php_make_changes(&preview);
        assert_eq!(app.doc().text.all_lines()[3].as_ref(), "$c = $s[0];");
        let preview = app.php_plan(Tool::Headers, Scope::File, false);
        assert_eq!(preview.files[0].changes.len(), 3);
        app.php_make_changes(&preview);
        assert!(!app.doc().text.format.has_bom, "the mark goes");
        let lines: Vec<String> = app.doc().text.all_lines().iter().map(|l| l.to_string()).collect();
        assert_eq!(lines.first().map(String::as_str), Some("<?php $a = array(1);"));
        assert_eq!(lines.last().map(String::as_str), Some(""), "{lines:?}");
        assert_eq!(lines[lines.len() - 2], "$r = mysql_query($q);");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An old Latin-1 file on disk, and one open as Windows-1252, made UTF-8.
    #[test]
    fn old_files_are_made_utf8() {
        let (mut app, _ctx) = app_with("");
        let dir = folder("utf8");
        std::fs::write(dir.join("old.php"), b"<?php echo 'caf\xE9';\n").unwrap();
        std::fs::write(dir.join("quotes.php"), b"<?php echo '\x93hi\x94';\n").unwrap();
        std::fs::write(dir.join("new.php"), "<?php echo 'caf\u{e9}';\n").unwrap();
        assert!(app.open_path(&dir.join("quotes.php")));
        let preview = app.php_plan(Tool::Utf8, Scope::Project, false);
        let names: Vec<(&str, bool)> = preview.files.iter().map(|p| (p.name.as_str(), p.legacy.as_ref().unwrap().windows)).collect();
        assert_eq!(names, [("old.php", false), ("quotes.php", true)], "new.php is UTF-8 already");
        app.php_make_changes(&preview);
        assert_eq!(std::fs::read_to_string(dir.join("old.php")).unwrap(), "<?php echo 'caf\u{e9}';\n");
        assert!(app.doc().text.format.charset.is_unicode(), "the open one is converted in its tab");
        assert_eq!(app.doc().text.all_lines()[0].as_ref(), "<?php echo '\u{201C}hi\u{201D}';");
        assert!(app.php_plan(Tool::Utf8, Scope::Project, false).files.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Saving a PHP file checks it with php -l, marking a problem beside its line, and the mark goes once it's put
    /// right. F9 on a script runs it, with what it prints in the Output panel.
    #[test]
    fn saving_checks_and_f9_runs() {
        if php_here().is_none() {
            eprintln!("PHP isn't installed here, so saving and running weren't tried with it.");
            return;
        }
        let (mut app, _ctx) = app_with("");
        let dir = folder("save");
        let path = dir.join("script.php");
        std::fs::write(&path, "<?php\n$a = 6 * 7\necho \"answer $a\\n\";\n").unwrap();
        assert!(app.open_path(&path));
        let wait = |app: &mut Limoniter| {
            let began = Instant::now();
            while app.php_tick() && !app.php.linting.is_empty() && began.elapsed() < Duration::from_secs(20) {
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let index = app.active;
        assert!(app.save(index));
        wait(&mut app);
        let marks = app.doc().text.intel.as_ref().map(|i| i.build_problems.clone()).unwrap_or_default();
        assert_eq!(marks.len(), 1, "{marks:?}");
        assert_eq!(marks[0].0, 2, "on the line PHP said, from 0");
        app.doc().text.set_line_keeping_cursor(1, vec!["$a = 6 * 7;".into()]);
        assert!(app.save(index));
        wait(&mut app);
        assert!(app.doc().text.intel.as_ref().is_none_or(|i| i.build_problems.is_empty()), "put right, the mark goes");
        assert!(app.php_run_on_f9(), "a PHP file is run by F9");
        let began = Instant::now();
        while app.check_run_tool() && began.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.script_output.iter().any(|l| l == "answer 42"), "{:?}", app.script_output);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn php_here() -> Option<PathBuf> {
        crate::emulators::find_program(&["php"], &[])
    }

    #[test]
    fn addresses_on_the_server_are_made_from_the_folder() {
        let folder = Path::new("/w/site");
        assert_eq!(address_of(8000, folder, Some(Path::new("/w/site/admin/my page.php"))), "http://localhost:8000/admin/my%20page.php");
        assert_eq!(address_of(8001, folder, None), "http://localhost:8001/");
        assert_eq!(PhpState::from_settings("").to_settings(), "", "on unless turned off");
        assert!(!PhpState::from_settings("php_lint_on_save=false\n").lint_on_save);
        assert!(free_port().is_some_and(|p| p >= 8000));
    }

    #[test]
    fn php_lint_finds_a_real_problem() {
        let Some(php) = php_here() else {
            eprintln!("PHP isn't installed here, so php -l wasn't tried.");
            return;
        };
        let problems = lint_with(&php, None, Some(b"<?php\n$a = 1\necho $a;\n")).unwrap();
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].line, 3);
        assert!(problems[0].message.contains("syntax error"), "{:?}", problems);
        assert!(lint_with(&php, None, Some(b"<?php\necho 1;\n")).unwrap().is_empty());
        let dir = std::env::temp_dir().join(format!("limoniter-php-lint-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("dup.php");
        std::fs::write(&file, "<?php\nfunction f() {}\nfunction f() {}\n").unwrap();
        let problems = lint_with(&php, Some(&file), None).unwrap();
        assert_eq!(problems.iter().map(|p| p.line).collect::<Vec<_>>(), [3], "{problems:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The tools' changes are tried for real: each file PHP reads the same way before and after (php -l), and the
    /// pages it serves come out the same.
    #[test]
    fn changed_files_still_mean_the_same_to_php() {
        let Some(php) = php_here() else {
            eprintln!("PHP isn't installed here, so the changes weren't checked with it.");
            return;
        };
        let samples: [&[u8]; 3] = [
            b"<?php\n$a = array(1, array('x' => 'array(', 'y' => array()), \"(\" . ')');\n$s = <<<EOT\narray(1)\nEOT;\necho json_encode($a), $s, \"\\n\";\n",
            b"<?php\r\n$a = array(\r\n  'k' => array(1, 2),\r\n);\r\necho count($a['k']);\r\n?>\r\n\r\n",
            b"\xEF\xBB\xBF<?php\n$v = array(3);\necho $v[0] ?>\n",
        ];
        for sample in samples {
            let run = |bytes: &[u8]| {
                let mut child = crate::platform::command(&php).args(["-n", "-d", "display_errors=1"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
                std::io::Write::write_all(&mut child.stdin.take().unwrap(), bytes).unwrap();
                child.wait_with_output().unwrap().stdout
            };
            let arrays = php_tools::apply(sample, &php_tools::short_arrays(sample));
            assert_ne!(arrays, sample);
            assert_eq!(run(&arrays), run(sample), "array() to [] changes nothing PHP does: {}", String::from_utf8_lossy(&arrays));
            let headers = php_tools::apply(&arrays, &php_tools::headers_fixes(&arrays));
            assert!(lint_with(&php, None, Some(&headers)).unwrap().is_empty(), "{}", String::from_utf8_lossy(&headers));
            let out = run(&headers);
            assert!(!out.starts_with(b"\xEF\xBB\xBF") && !out.ends_with(b"\n\n"), "nothing's sent before or after the page");
        }
    }

    #[cfg(unix)]
    #[test]
    fn php_serves_a_folder() {
        let Some(php) = php_here() else {
            eprintln!("PHP isn't installed here, so php -S wasn't tried.");
            return;
        };
        use std::io::{Read, Write};
        let dir = std::env::temp_dir().join(format!("limoniter-php-serve-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/page.php"), "<p><?php echo 6 * 7; ?></p>").unwrap();
        let port = free_port().unwrap();
        let line = format!("{} -S localhost:{port} -t {}", quoted(&php.display().to_string()), quoted(&dir.display().to_string()));
        let mut job = super::super::file_tools::start_run(&line, Some(&dir), || {}).unwrap();
        let began = Instant::now();
        let mut stream = loop {
            match std::net::TcpStream::connect(("localhost", port)) {
                Ok(stream) => break stream,
                Err(_) if began.elapsed() < Duration::from_secs(5) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => panic!("the server didn't start: {e}"),
            }
        };
        let address = address_of(port, &dir, Some(&dir.join("sub/page.php")));
        let path = address.split_once(&format!(":{port}")).unwrap().1;
        write!(stream, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        assert!(answer.contains("<p>42</p>"), "{answer}");
        job.stop();
        let began = Instant::now();
        while job.take_output().1.is_none() && began.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
