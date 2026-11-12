//! An assistant for code: Claude Code or Codex, run as the person's own command-line tool, to fix, clean up,
//! explain or write code. Limoniter has no account or key of its own: it runs the tool the person has installed and
//! signed in to, hands it the part of the file and what's wanted, and shows what comes back. Nothing goes in until
//! the person has seen it and said so, and it then comes off with one Undo.
//!
//! The tool is run so that it can read the project for context but can't change files or run commands: Claude Code
//! with only its reading tools, and Codex in its read-only sandbox. Both commands can be changed in the box.

use super::*;
use crate::region_history;
use std::io::{Read, Write};
use std::ops::Range;

pub(super) const CLAUDE_COMMAND: &str = r#"claude -p --no-session-persistence --tools "Read,Grep,Glob""#;
pub(super) const CODEX_COMMAND: &str = "codex exec --sandbox read-only --skip-git-repo-check -";
/// Lines either side of the part, so the tool can see what it's in.
const CONTEXT_LINES: usize = 120;
/// The most of a file that's sent as the part itself.
const MOST_PART_LINES: usize = 3_000;
const MOST_REPLY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tool {
    Claude,
    Codex,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Claude => "Claude Code",
            Tool::Codex => "Codex",
        }
    }

    /// Who the tool sends what it's given to.
    fn company(self) -> &'static str {
        match self {
            Tool::Claude => "Anthropic",
            Tool::Codex => "OpenAI",
        }
    }

    fn program(self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
        }
    }

    fn how_to_get(self) -> &'static str {
        match self {
            Tool::Claude => "Claude Code isn't installed. It can be, from claude.com/claude-code, and then signed in to by running claude once.",
            Tool::Codex => "Codex isn't installed. It can be, with: npm install -g @openai/codex, and then signed in to by running codex once.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Task {
    Fix,
    CleanUp,
    Write,
    Explain,
    Ask,
}

impl Task {
    pub const ALL: [Task; 5] = [Task::Fix, Task::CleanUp, Task::Write, Task::Explain, Task::Ask];

    pub fn label(self) -> &'static str {
        match self {
            Task::Fix => "Fix",
            Task::CleanUp => "Clean up",
            Task::Write => "Write code",
            Task::Explain => "Explain",
            Task::Ask => "Ask",
        }
    }

    /// Whether what comes back is code to put in, rather than words to read.
    pub fn changes_text(self) -> bool {
        matches!(self, Task::Fix | Task::CleanUp | Task::Write)
    }

    fn needs_words(self) -> bool {
        matches!(self, Task::Write | Task::Ask)
    }

    fn instruction(self) -> &'static str {
        match self {
            Task::Fix => "Find and fix any bugs in the part. If it's already right, give it back unchanged.",
            Task::CleanUp => "Clean up the part: make it clearer and tidier, without changing what it does.",
            Task::Write => "Write the code that's asked for, to go in place of the part. If the part is empty, the code goes between the lines before and after it.",
            Task::Explain => "Explain what the part does, briefly and in plain words.",
            Task::Ask => "Answer the question about the part, briefly and in plain words.",
        }
    }
}

/// Which tool, and the command for each, as kept in the settings file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Setup {
    pub tool: Tool,
    pub claude: String,
    pub codex: String,
}

impl Setup {
    pub fn from_settings(settings: &str) -> Self {
        let one = |name: &str| list_setting(settings, name).into_iter().next().filter(|value| !value.trim().is_empty());
        Self {
            tool: if one("assistant_tool").as_deref() == Some("codex") { Tool::Codex } else { Tool::Claude },
            claude: one("assistant_claude").unwrap_or_else(|| CLAUDE_COMMAND.into()),
            codex: one("assistant_codex").unwrap_or_else(|| CODEX_COMMAND.into()),
        }
    }

    pub fn to_settings(&self) -> String {
        let mut out = format!("assistant_tool={}\n", if self.tool == Tool::Codex { "codex" } else { "claude" });
        if self.claude != CLAUDE_COMMAND {
            out += &format!("assistant_claude={}\n", self.claude.replace(['\r', '\n'], " "));
        }
        if self.codex != CODEX_COMMAND {
            out += &format!("assistant_codex={}\n", self.codex.replace(['\r', '\n'], " "));
        }
        out
    }

    pub fn command(&self) -> &str {
        match self.tool {
            Tool::Claude => &self.claude,
            Tool::Codex => &self.codex,
        }
    }
}

/// Whether a program can be found on the PATH, as the Command Prompt would find it.
fn installed(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let endings: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat", ".com"] } else { &[""] };
    std::env::split_paths(&path).any(|dir| endings.iter().any(|ext| dir.join(format!("{program}{ext}")).is_file()))
}

/// What's said to the tool: the task, what was asked, and the part with some lines around it.
pub(super) fn prompt(task: Task, words: &str, file: &str, language: &str, before: &[String], part: &[String], after: &[String], first_line: usize) -> String {
    let fence = |lines: &[String]| {
        // A fence longer than any run of backticks in the text, so the text can't end it.
        let longest = lines.iter().flat_map(|line| line.split(|c| c != '`')).map(str::len).max().unwrap_or(0);
        let marks = "`".repeat(longest.max(2) + 1);
        format!("{marks}\n{}{}{marks}\n", lines.join("\n"), if lines.is_empty() { "" } else { "\n" })
    };
    let lines = if part.is_empty() { format!("at line {first_line}") } else { format!("lines {} to {}", first_line, first_line + part.len() - 1) };
    let mut out = format!("You're helping someone in a text editor. They've chosen a part of {file} ({language}), {lines}.\n\n{}\n", task.instruction());
    if !words.trim().is_empty() {
        out += &format!("\nWhat they said: {}\n", words.trim());
    }
    if task.changes_text() {
        out += "\nReply with the code that goes in place of the part, complete, in one fenced code block, and nothing else. Keep its indentation and style. Change only the part: the lines around it are there so you can see what it's in.\n";
    } else {
        out += "\nDon't rewrite the code unless they ask for that.\n";
    }
    if !before.is_empty() {
        out += &format!("\nThe lines before the part, for context only:\n{}", fence(before));
    }
    out += &format!("\nThe part:\n{}", fence(part));
    if !after.is_empty() {
        out += &format!("\nThe lines after the part, for context only:\n{}", fence(after));
    }
    out
}

/// The code in a reply: what's in its first fenced block, or the whole reply if it hasn't one.
pub(super) fn code_from_reply(reply: &str) -> Vec<String> {
    let lines: Vec<&str> = reply.lines().collect();
    let opening = lines.iter().position(|line| line.trim_start().starts_with("```"));
    let body: Vec<&str> = match opening {
        Some(start) => {
            let marks: String = lines[start].trim_start().chars().take_while(|c| *c == '`').collect();
            let end = lines[start + 1..].iter().position(|line| line.trim() == marks || line.trim().starts_with(marks.as_str()) && line.trim().chars().all(|c| c == '`')).map_or(lines.len(), |at| start + 1 + at);
            lines[start + 1..end].to_vec()
        }
        None => {
            let trimmed = reply.trim_matches('\n');
            trimmed.lines().collect()
        }
    };
    body.into_iter().map(|line| line.trim_end_matches('\r').to_string()).collect()
}

/// The tool at work on a part of a file.
pub(super) struct AssistJob {
    result: Arc<Mutex<Option<Result<String, String>>>>,
    process: u32,
    started: std::time::Instant,
    pub tool: Tool,
    pub task: Task,
    doc: u64,
    range: Range<usize>,
    /// The text as it was sent, so the part can be found again if it's been edited meanwhile.
    text: Vec<Arc<str>>,
    old: Vec<String>,
}

/// Runs a command line with the prompt on its input, and keeps what it prints.
fn start(line: &str, folder: Option<&Path>, input: String, wake: impl Fn() + Send + 'static) -> Result<(Arc<Mutex<Option<Result<String, String>>>>, u32), String> {
    let mut command = crate::platform::shell_command(line);
    if let Some(folder) = folder.filter(|folder| folder.is_dir()) {
        command.current_dir(folder);
    }
    let mut child = command.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().map_err(|e| format!("Couldn't start it.\n\n{e}"))?;
    let process = child.id();
    let mut stdin = child.stdin.take();
    std::thread::spawn(move || {
        if let Some(stdin) = &mut stdin {
            // A tool that doesn't read its input closes it, which is no matter.
            let _ = stdin.write_all(input.as_bytes());
        }
    });
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    std::thread::spawn(move || {
        let mut stderr = child.stderr.take();
        let errors = std::thread::spawn(move || {
            let mut text = Vec::new();
            if let Some(stderr) = &mut stderr {
                let _ = stderr.take(MOST_REPLY_BYTES as u64).read_to_end(&mut text);
            }
            String::from_utf8_lossy(&text).into_owned()
        });
        let mut reply = Vec::new();
        if let Some(stdout) = &mut child.stdout {
            let _ = stdout.take(MOST_REPLY_BYTES as u64).read_to_end(&mut reply);
        }
        let status = child.wait();
        let errors = errors.join().unwrap_or_default();
        let reply = String::from_utf8_lossy(&reply).into_owned();
        let answer = match status {
            Ok(status) if status.success() && !reply.trim().is_empty() => Ok(reply),
            Ok(status) => {
                let said = if errors.trim().is_empty() { reply.trim().to_string() } else { errors.trim().to_string() };
                let tail: String = said.chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect();
                Err(format!("It stopped without an answer{}.{}", status.code().map_or_else(String::new, |code| format!(", with exit code {code}")), if tail.is_empty() { String::new() } else { format!("\n\nIt said:\n{tail}") }))
            }
            Err(e) => Err(format!("Couldn't wait for it: {e}")),
        };
        *out.lock().unwrap() = Some(answer);
        wake();
    });
    Ok((result, process))
}

/// Each line of a change, marked '-' for taken away, '+' for put in, or ' ' for left as it was.
pub(super) fn diff_rows<'a>(old: &'a [String], new: &'a [String]) -> Vec<(char, &'a str)> {
    let mut rows = Vec::new();
    for op in similar::capture_diff_slices(similar::Algorithm::Myers, old, new) {
        let (o, n) = (op.old_range(), op.new_range());
        match op.tag() {
            similar::DiffTag::Equal => rows.extend(old[o].iter().map(|line| (' ', line.as_str()))),
            _ => {
                rows.extend(old[o].iter().map(|line| ('-', line.as_str())));
                rows.extend(new[n].iter().map(|line| ('+', line.as_str())));
            }
        }
    }
    rows
}

fn kill_tree(process: u32) {
    crate::platform::kill_tree(process);
}

/// What the person is choosing in the box.
pub(super) struct AssistBox {
    pub task: Task,
    pub words: String,
    pub show_commands: bool,
    pub focus: bool,
    pub error: String,
}

/// What came back, for the person to look at before it goes in.
pub(super) struct Review {
    pub tool: Tool,
    pub doc: u64,
    pub range: Range<usize>,
    text: Vec<Arc<str>>,
    pub old: Vec<String>,
    pub new: Vec<String>,
}

impl Limoniter {
    /// The lines the assistant works on: those selected, or the function or paragraph the cursor is in. For writing
    /// with nothing selected, a blank line the cursor is on, or the place after its line.
    fn assist_range(&self, task: Task) -> Range<usize> {
        let text = &self.docs[self.active].text;
        if task == Task::Write && !text.has_selection() {
            let line = text.caret_line;
            return if text.line(line).trim().is_empty() { line..line + 1 } else { line + 1..line + 1 };
        }
        region_panel::chosen_lines(text)
    }

    pub(super) fn begin_assist(&mut self, task: Task) {
        if self.assist_job.is_some() {
            self.message("Assistant", "The assistant is still working on the last request. It can be stopped from the note at the bottom right.".into());
            return;
        }
        self.dialog = Dialog::Assist(AssistBox { task, words: String::new(), show_commands: false, focus: true, error: String::new() });
    }

    /// Sends the part to the tool. Says why not if it can't.
    pub(super) fn send_to_assistant(&mut self, task: Task, words: &str) -> Result<(), String> {
        if task.needs_words() && words.trim().is_empty() {
            return Err(if task == Task::Write { "Say what code to write.".into() } else { "Type the question.".into() });
        }
        let tool = self.assistant.tool;
        let line = self.assistant.command().trim().to_string();
        if line.is_empty() {
            return Err("There's no command to run. Put the usual one back under Commands.".into());
        }
        if line.split_whitespace().next() == Some(tool.program()) && !installed(tool.program()) {
            return Err(tool.how_to_get().into());
        }
        let doc = &self.docs[self.active];
        if doc.hex_mode || doc.text.is_disk_backed() {
            return Err("The assistant works on text, and on files small enough to be held in memory.".into());
        }
        let range = self.assist_range(task);
        if range.len() > MOST_PART_LINES {
            return Err(format!("That's {} lines. Select a part of at most {MOST_PART_LINES} lines.", range.len()));
        }
        let text = &doc.text;
        let all = text.all_lines();
        let readable = |lines: &[Arc<str>]| -> Vec<String> { lines.iter().map(|line| text.format.readable_text(line)).collect() };
        let before = readable(&all[range.start.saturating_sub(CONTEXT_LINES)..range.start]);
        let part = readable(&all[range.clone()]);
        let after = readable(&all[range.end..(range.end + CONTEXT_LINES).min(all.len())]);
        let language = doc.language_name();
        let input = prompt(task, words, &doc.name(), if language == "No highlighting" { "plain text" } else { &language }, &before, &part, &after, range.start + 1);
        let folder = doc.path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        let wake = self.waker.clone();
        let (result, process) = start(&line, folder.as_deref(), input, move || {
            if let Some(ctx) = &wake {
                ctx.request_repaint();
            }
        })?;
        self.assist_job = Some(AssistJob { result, process, started: std::time::Instant::now(), tool, task, doc: doc.id, range, text: all, old: part });
        Ok(())
    }

    pub(super) fn stop_assistant(&mut self) {
        if let Some(job) = self.assist_job.take() {
            kill_tree(job.process);
        }
    }

    /// Takes in the answer once it's there. True while the tool is still working.
    pub(super) fn check_assistant(&mut self) -> bool {
        let Some(job) = &self.assist_job else {
            return false;
        };
        let Some(answer) = job.result.lock().unwrap().take() else {
            return true;
        };
        let job = self.assist_job.take().expect("there's a job");
        match answer {
            Err(problem) => self.message(job.tool.name(), problem),
            Ok(reply) if !job.task.changes_text() => {
                let place = if job.old.is_empty() { format!("line {}", job.range.start + 1) } else { format!("lines {} to {}", job.range.start + 1, job.range.end) };
                self.script_print(format!("{}, on {place}:", job.tool.name()), true);
                self.script_print(String::new(), false);
                for line in reply.trim_end().lines() {
                    self.script_print(line.to_string(), false);
                }
            }
            Ok(reply) => {
                let new = code_from_reply(&reply);
                self.dialog = Dialog::AssistReview(Review { tool: job.tool, doc: job.doc, range: job.range, text: job.text, old: job.old, new });
            }
        }
        false
    }

    /// Puts what came back in place of the part, wherever the part is now.
    pub(super) fn accept_review(&mut self, review: &Review) -> Result<(), String> {
        let index = self.docs.iter().position(|doc| doc.id == review.doc).ok_or("That file has been closed.")?;
        let doc = &self.docs[index];
        if doc.text.read_only {
            return Err("The file is read-only now.".into());
        }
        if doc.hex_mode || doc.is_busy() {
            return Err("The file is in hex mode, or busy. Try again in a moment.".into());
        }
        let now = doc.text.all_lines();
        let range = if now == review.text { review.range.clone() } else { region_history::diff(&review.text, &now).map_range(review.range.clone()).range };
        let lines = review.new.iter().map(|line| Arc::from(line.as_str())).collect();
        self.active = index;
        self.doc().text.splice_lines(range.start, range.end, lines);
        self.pending_focus = true;
        Ok(())
    }

    /// The note that the tool is working, with a way to stop it.
    pub(super) fn draw_assistant_note(&mut self, ctx: &Context) {
        let Some(job) = &self.assist_job else {
            return;
        };
        let seconds = job.started.elapsed().as_secs();
        let words = format!("{} is working on {}\u{2026} {seconds}s", job.tool.name(), job.task.label().to_lowercase());
        let mut stop = false;
        egui::Area::new(egui::Id::unique("assistant-note")).order(egui::Order::Foreground).anchor(egui::Align2::RIGHT_BOTTOM, [-24.0, -44.0]).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(words);
                    stop = ui.button("Stop").clicked();
                });
            });
        });
        ctx.request_repaint_after(Duration::from_millis(500));
        if stop {
            self.stop_assistant();
        }
    }

    pub(super) fn assistant_dialogs(&mut self, ctx: &Context) -> bool {
        match &self.dialog {
            Dialog::Assist(_) => self.assist_box(ctx),
            Dialog::AssistReview(_) => self.review_box(ctx),
            _ => return false,
        }
        true
    }

    fn assist_box(&mut self, ctx: &Context) {
        let Dialog::Assist(state) = &self.dialog else {
            return;
        };
        let range = self.assist_range(state.task);
        let doc = &self.docs[self.active];
        let name = doc.name();
        let usable = !self.no_file() && !doc.hex_mode;
        let tool = self.assistant.tool;
        let found = [Tool::Claude, Tool::Codex].map(|tool| installed(tool.program()));
        let (escape, enter) = ctx.input(|i| (i.key_pressed(Key::Escape), i.key_pressed(Key::Enter) && !i.modifiers.shift));
        let Dialog::Assist(state) = &mut self.dialog else {
            return;
        };
        let (mut send, mut close) = (false, escape);
        let setup = &mut self.assistant;
        egui::Window::new("Assistant").collapsible(false).resizable(false).fixed_size(egui::vec2(560.0, 0.0)).anchor(egui::Align2::CENTER_TOP, [0.0, 70.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for task in Task::ALL {
                    if ui.add(egui::Button::selectable(state.task == task, task.label())).clicked() {
                        state.task = task;
                        state.error.clear();
                        state.focus = true;
                    }
                }
            });
            ui.add_space(6.0);
            let place = if range.is_empty() { format!("the place after line {}", range.start) } else { format!("lines {} to {}", range.start + 1, range.end) };
            ui.label(RichText::new(format!("Works on {place} of {name}.")).color(theme::text_dim()));
            let hint = match state.task {
                Task::Fix => "Anything it should know, such as what goes wrong (optional)",
                Task::CleanUp => "Anything it should keep or change (optional)",
                Task::Write => "What the code should do",
                Task::Explain => "Anything in particular to explain (optional)",
                Task::Ask => "The question",
            };
            let field = ui.add(egui::TextEdit::multiline(&mut state.words).hint_text(hint).desired_rows(3).desired_width(f32::INFINITY));
            if state.focus {
                field.request_focus();
                state.focus = false;
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Using");
                for (index, each) in [Tool::Claude, Tool::Codex].into_iter().enumerate() {
                    let label = if found[index] { each.name().to_string() } else { format!("{} (not installed)", each.name()) };
                    ui.radio_value(&mut setup.tool, each, label);
                }
            });
            ui.label(RichText::new(format!("The part, and up to {CONTEXT_LINES} lines either side of it, go to {}, which sends them to {}. It can read the files in this one's folder, but can't change anything or run commands. What comes back is shown before anything changes.", tool.name(), tool.company())).color(theme::text_faint()).size(11.5));
            ui.add_space(4.0);
            ui.checkbox(&mut state.show_commands, "Commands");
            if state.show_commands {
                egui::Grid::new("assistant-commands").num_columns(2).show(ui, |ui| {
                    ui.label("Claude Code");
                    ui.add(egui::TextEdit::singleline(&mut setup.claude).font(egui::TextStyle::Monospace).desired_width(420.0));
                    ui.end_row();
                    ui.label("Codex");
                    ui.add(egui::TextEdit::singleline(&mut setup.codex).font(egui::TextStyle::Monospace).desired_width(420.0));
                    ui.end_row();
                });
                ui.label(RichText::new("Each is run in the file's folder, with the request on its input, and what it prints is the answer.").color(theme::text_faint()).size(11.5));
                if ui.button("Put the usual commands back").clicked() {
                    setup.claude = CLAUDE_COMMAND.into();
                    setup.codex = CODEX_COMMAND.into();
                }
            }
            if !state.error.is_empty() {
                ui.add_space(4.0);
                ui.label(RichText::new(state.error.as_str()).color(theme::rainbow_mistake()));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                send = ui.add_enabled(usable, Button::new(RichText::new(format!("Send to {}", setup.tool.name())).strong())).on_hover_text("Or press Enter").clicked();
                close |= ui.button("Cancel").clicked();
            });
        });
        let (task, words) = (state.task, state.words.clone());
        if (send || (enter && usable)) && !close {
            match self.send_to_assistant(task, &words) {
                Ok(()) => self.close_dialog(),
                Err(problem) => {
                    if let Dialog::Assist(state) = &mut self.dialog {
                        state.error = problem;
                    }
                }
            }
        } else if close {
            self.close_dialog();
        }
    }

    fn review_box(&mut self, ctx: &Context) {
        let Dialog::AssistReview(review) = &self.dialog else {
            return;
        };
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let (mut accept, mut copy, mut close) = (false, false, escape);
        let rows = diff_rows(&review.old, &review.new);
        let changed = rows.iter().any(|(mark, _)| *mark != ' ');
        egui::Window::new(format!("{} suggests", review.tool.name())).collapsible(false).resizable(true).default_size(egui::vec2(760.0, 480.0)).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            let place = if review.old.is_empty() { format!("New code after line {}", review.range.start) } else { format!("Lines {} to {}", review.range.start + 1, review.range.end) };
            ui.label(RichText::new(if changed { place } else { format!("{place}: it suggests no change") }).color(theme::text_dim()));
            ui.add_space(4.0);
            egui::ScrollArea::both().max_height(520.0).auto_shrink([false, true]).show(ui, |ui| {
                for (mark, line) in &rows {
                    let (mark, colour, back) = match mark {
                        '-' => ("\u{2212}", theme::rainbow_mistake(), theme::compare_removed()),
                        '+' => ("+", theme::change_saved(), theme::compare_added()),
                        _ => (" ", theme::text_dim(), egui::Color32::TRANSPARENT),
                    };
                    let text = RichText::new(format!("{mark} {line}")).monospace().size(12.5).color(colour).background_color(back);
                    ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend));
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                accept = ui.add_enabled(changed, Button::new(RichText::new("Put it in").strong())).on_hover_text("One Undo takes it out again").clicked();
                copy = ui.button("Copy it").clicked();
                close |= ui.button("Leave it").clicked();
            });
        });
        if copy {
            ctx.copy_text(review.new.join("\r\n"));
        }
        if accept {
            let Dialog::AssistReview(review) = std::mem::replace(&mut self.dialog, Dialog::None) else {
                return;
            };
            match self.accept_review(&review) {
                Ok(()) => self.close_dialog(),
                Err(problem) => self.message(review.tool.name(), problem),
            }
        } else if close {
            self.close_dialog();
        }
    }

    pub(super) fn assistant_screenshot_step(&mut self, step: &str) {
        let text = &mut self.doc().text;
        text.set_content(["function total(items) {", "  var sum = 0", "  for (var i = 0; i <= items.length; i++) {", "    sum += items[i].price", "  }", "  return sum", "}"].map(String::from).to_vec());
        text.move_caret(2, 4, false);
        if step == "assistant" {
            self.begin_assist(Task::Fix);
            if let Dialog::Assist(state) = &mut self.dialog {
                state.words = "It gives NaN for some lists".into();
            }
        } else {
            let old: Vec<String> = (0..7).map(|at| self.docs[self.active].text.line(at).to_string()).collect();
            let new = ["function total(items) {", "  let sum = 0;", "  for (let i = 0; i < items.length; i++) {", "    sum += items[i].price;", "  }", "  return sum;", "}"].map(String::from).to_vec();
            let text = self.docs[self.active].text.all_lines();
            self.dialog = Dialog::AssistReview(Review { tool: Tool::Claude, doc: self.docs[self.active].id, range: 0..7, text, old, new });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn the_prompt_holds_the_task_the_part_and_its_surroundings() {
        let text = prompt(Task::Fix, "it crashes", "page.js", "JavaScript", &strings(&["// above"]), &strings(&["let a = b;", "```"]), &strings(&["// below"]), 12);
        assert!(text.contains("page.js (JavaScript), lines 12 to 13"));
        assert!(text.contains("fix any bugs"));
        assert!(text.contains("What they said: it crashes"));
        assert!(text.contains("one fenced code block"));
        assert!(text.contains("````\nlet a = b;\n```\n````"), "a fence the text can't end: {text}");
        assert!(text.contains("for context only:\n```\n// above\n```"));
        let explain = prompt(Task::Explain, "", "a.rs", "Rust", &[], &strings(&["x"]), &[], 1);
        assert!(!explain.contains("What they said") && !explain.contains("fenced code block"));
    }

    #[test]
    fn the_code_is_taken_from_the_first_fenced_block() {
        assert_eq!(code_from_reply("Here it is:\n```rust\nfn a() {}\n  b();\n```\nThat fixes it."), ["fn a() {}", "  b();"]);
        assert_eq!(code_from_reply("````\n```\ninside\n```\n````"), ["```", "inside", "```"], "a longer fence holds shorter ones");
        assert_eq!(code_from_reply("\nplain answer\nsecond line\n"), ["plain answer", "second line"]);
        assert_eq!(code_from_reply("```\r\nwindows\r\n```\r\n"), ["windows"]);
    }

    #[test]
    fn the_tool_and_its_commands_are_remembered() {
        let usual = Setup::from_settings("");
        assert_eq!(usual.tool, Tool::Claude);
        assert_eq!(usual.command(), CLAUDE_COMMAND);
        assert_eq!(usual.to_settings(), "assistant_tool=claude\n", "the usual commands aren't written down");
        let changed = Setup { tool: Tool::Codex, claude: "claude -p".into(), codex: CODEX_COMMAND.into() };
        assert_eq!(Setup::from_settings(&changed.to_settings()), changed);
    }

    fn app_with_text(lines: &[&str]) -> (Limoniter, Context) {
        let mut app = Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png")));
        app.new_document();
        app.doc().text.set_content(strings(lines));
        (app, Context::default())
    }

    fn wait(app: &mut Limoniter, ctx: &Context) {
        for _ in 0..1000 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.draw_frame(ui));
            output.textures_delta.clear();
            if app.assist_job.is_none() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("no answer in time");
    }

    /// A stand-in for the tool, which only prints an answer kept in a file. Nothing goes anywhere.
    fn stand_in(reply: &str) -> (PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("limoniter-assistant-{}-{}", std::process::id(), reply.len()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("reply.txt");
        std::fs::write(&file, reply).unwrap();
        (dir, format!("{} \"{}\"", if cfg!(windows) { "type" } else { "cat" }, file.display()))
    }

    #[test]
    fn what_comes_back_is_shown_and_goes_in_only_when_accepted() {
        let (dir, command) = stand_in("Fixed:\n```js\nfunction f() {\n  return 1;\n}\n```\n");
        let (mut app, ctx) = app_with_text(&["// top", "function f() {", "  return 1", "}", "// end"]);
        app.assistant.claude = command;
        app.doc().text.move_caret(2, 0, false);
        app.send_to_assistant(Task::Fix, "").unwrap();
        wait(&mut app, &ctx);
        let Dialog::AssistReview(review) = &app.dialog else {
            panic!("the answer is shown first");
        };
        assert_eq!(review.range, 1..4, "the function the cursor was in");
        assert_eq!(review.new, ["function f() {", "  return 1;", "}"]);
        assert_eq!(app.docs[app.active].text.line(2).as_ref(), "  return 1", "nothing has changed yet");
        // Typing above it meanwhile doesn't put it in the wrong place.
        app.doc().text.splice_lines(0, 0, vec![Arc::from("// added")]);
        let Dialog::AssistReview(review) = std::mem::replace(&mut app.dialog, Dialog::None) else {
            unreachable!()
        };
        app.accept_review(&review).unwrap();
        let lines: Vec<String> = app.docs[app.active].text.all_lines().iter().map(|line| line.to_string()).collect();
        assert_eq!(lines, ["// added", "// top", "function f() {", "  return 1;", "}", "// end"]);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.run(Command::Undo, ui));
        output.textures_delta.clear();
        assert_eq!(app.docs[app.active].text.line(3).as_ref(), "  return 1", "one Undo takes it out");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_explanation_goes_to_the_output_and_a_failure_says_what_happened() {
        let (dir, command) = stand_in("It adds one to x.\n");
        let (mut app, ctx) = app_with_text(&["x += 1;"]);
        app.assistant.claude = command;
        app.send_to_assistant(Task::Explain, "").unwrap();
        wait(&mut app, &ctx);
        assert!(app.script_output.iter().any(|line| line == "It adds one to x."), "{:?}", app.script_output);
        assert!(app.docs[app.active].text.line(0).as_ref() == "x += 1;");
        app.assistant.claude = "echo not signed in 1>&2 & exit 3".into();
        app.send_to_assistant(Task::Fix, "").unwrap();
        wait(&mut app, &ctx);
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("exit code 3") && body.contains("not signed in")));
        assert!(app.send_to_assistant(Task::Write, "  ").unwrap_err().contains("what code"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
