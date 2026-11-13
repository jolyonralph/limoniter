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
    /// Its entry in the history.
    exchange: u64,
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
/// The Assistant panel: the history, and the request being written.
pub(super) struct AssistPanel {
    pub open: bool,
    pub task: Task,
    pub words: String,
    pub show_commands: bool,
    pub focus: bool,
    pub error: String,
}

impl Default for AssistPanel {
    fn default() -> Self {
        AssistPanel { open: false, task: Task::Fix, words: String::new(), show_commands: false, focus: false, error: String::new() }
    }
}

/// What came back, for the person to look at before it goes in.
pub(super) struct Review {
    pub tool: Tool,
    pub doc: u64,
    pub range: Range<usize>,
    text: Vec<Arc<str>>,
    pub old: Vec<String>,
    pub new: Vec<String>,
    /// Its entry in the history, and whether it's been put in already (so it's only to look at).
    pub exchange: Option<u64>,
    pub done: bool,
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

    /// Opens the Assistant panel, ready to ask for this.
    pub(super) fn begin_assist(&mut self, task: Task) {
        let panel = &mut self.assist_panel;
        panel.open = true;
        panel.task = task;
        panel.focus = true;
        panel.error.clear();
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
        let exchange = crate::app::assistant_log::Exchange {
            id: 0,
            when: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
            tool,
            task,
            words: words.trim().to_string(),
            file: doc.name(),
            path: doc.path.clone(),
            lines: range.clone(),
            state: crate::app::assistant_log::State::Working,
            reply: String::new(),
            old: part.clone(),
            new: Vec::new(),
            seconds: 0,
            doc: Some(doc.id),
            text: Some(all.clone()),
        };
        let exchange = self.assist_log.add(exchange);
        self.assist_job = Some(AssistJob { result, process, started: std::time::Instant::now(), tool, task, doc: doc.id, range, text: all, old: part, exchange });
        Ok(())
    }

    pub(super) fn stop_assistant(&mut self) {
        if let Some(job) = self.assist_job.take() {
            kill_tree(job.process);
            let seconds = job.started.elapsed().as_secs();
            self.assist_log.update(job.exchange, |e| {
                e.state = crate::app::assistant_log::State::Stopped;
                e.seconds = seconds;
            });
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
        let seconds = job.started.elapsed().as_secs();
        use crate::app::assistant_log::State;
        match &answer {
            Err(problem) => self.assist_log.update(job.exchange, |e| {
                e.state = State::Failed;
                e.reply.clone_from(problem);
                e.seconds = seconds;
            }),
            Ok(reply) if !job.task.changes_text() => self.assist_log.update(job.exchange, |e| {
                e.state = State::Answered;
                e.reply = reply.trim_end().to_string();
                e.seconds = seconds;
            }),
            Ok(reply) => {
                let new = code_from_reply(reply);
                self.assist_log.update(job.exchange, |e| {
                    e.state = State::Suggested;
                    e.reply = reply.trim_end().to_string();
                    e.new = new;
                    e.seconds = seconds;
                });
            }
        }
        match answer {
            // With the panel showing, it says so there.
            Err(problem) if !self.assist_panel.open => self.message(job.tool.name(), problem),
            Err(_) => {}
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
                self.dialog = Dialog::AssistReview(Review { tool: job.tool, doc: job.doc, range: job.range, text: job.text, old: job.old, new, exchange: Some(job.exchange), done: false });
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
            Dialog::AssistReview(_) => self.review_box(ctx),
            _ => return false,
        }
        true
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
                if review.done {
                    ui.label(RichText::new("This was put in.").color(theme::text_dim()));
                } else {
                    accept = ui.add_enabled(changed, Button::new(RichText::new("Put it in").strong())).on_hover_text("One Undo takes it out again").clicked();
                }
                copy = ui.button("Copy it").clicked();
                close |= ui.button(if review.done { "Close" } else { "Leave it" }).clicked();
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
                Ok(()) => {
                    self.close_dialog();
                    if let Some(id) = review.exchange {
                        self.assist_log.update(id, |e| e.state = crate::app::assistant_log::State::PutIn);
                    }
                }
                Err(problem) => self.message(review.tool.name(), problem),
            }
        } else if close {
            if let Dialog::AssistReview(review) = &self.dialog
                && let Some(id) = review.exchange.filter(|_| !review.done)
            {
                self.assist_log.update(id, |e| {
                    if e.state == crate::app::assistant_log::State::Suggested {
                        e.state = crate::app::assistant_log::State::Left;
                    }
                });
            }
            self.close_dialog();
        }
    }

    pub(super) fn assistant_screenshot_step(&mut self, step: &str) {
        let text = &mut self.doc().text;
        text.set_content(["function total(items) {", "  var sum = 0", "  for (var i = 0; i <= items.length; i++) {", "    sum += items[i].price", "  }", "  return sum", "}"].map(String::from).to_vec());
        text.move_caret(2, 4, false);
        if step == "assistant-history" {
            use crate::app::assistant_log::{Exchange, State};
            let file = self.docs[self.active].name();
            let made = |when: &str, tool: Tool, task: Task, words: &str, lines: Range<usize>, state: State, reply: &str, old: &[&str], new: &[&str], seconds: u64| Exchange { id: 0, when: when.into(), tool, task, words: words.into(), file: file.clone(), path: None, lines, state, reply: reply.into(), old: old.iter().map(|l| l.to_string()).collect(), new: new.iter().map(|l| l.to_string()).collect(), seconds, doc: None, text: None };
            for exchange in [
                made("2026-11-13 10:02", Tool::Claude, Task::Fix, "It gives NaN for some lists", 0..7, State::PutIn, "", &["  var sum = 0", "  for (var i = 0; i <= items.length; i++) {", "    sum += items[i].price"], &["  let sum = 0;", "  for (let i = 0; i < items.length; i++) {", "    sum += items[i].price;"], 14),
                made("2026-11-13 10:05", Tool::Claude, Task::Explain, "", 2..5, State::Answered, "The loop adds up each item's price. It used to run one step too far (<= instead of <), reading an item past the end of the list, whose price is undefined, so the total became NaN.", &[], &[], 9),
                made("2026-11-13 10:11", Tool::Codex, Task::CleanUp, "", 0..7, State::Left, "", &["function total(items) {", "  let sum = 0;"], &["const total = (items) =>", "  items.reduce((sum, item) => sum + item.price, 0);"], 21),
                made("2026-11-13 10:14", Tool::Codex, Task::Ask, "Would reduce be faster here?", 0..7, State::Failed, "codex: not signed in. Run codex login first.", &[], &[], 2),
            ] {
                self.assist_log.add(exchange);
            }
            self.begin_assist(Task::Write);
            self.assist_panel.words = "A function that returns the cheapest item".into();
            return;
        }
        if step == "assistant" {
            self.begin_assist(Task::Fix);
            self.assist_panel.words = "It gives NaN for some lists".into();
        } else {
            let old: Vec<String> = (0..7).map(|at| self.docs[self.active].text.line(at).to_string()).collect();
            let new = ["function total(items) {", "  let sum = 0;", "  for (let i = 0; i < items.length; i++) {", "    sum += items[i].price;", "  }", "  return sum;", "}"].map(String::from).to_vec();
            let text = self.docs[self.active].text.all_lines();
            self.dialog = Dialog::AssistReview(Review { tool: Tool::Claude, doc: self.docs[self.active].id, range: 0..7, text, old, new, exchange: None, done: false });
        }
    }
}

/// What the Assistant panel asks for.
enum PanelAct {
    GoTo(u64),
    Show(u64),
    Stop,
    Clear,
    Send,
}

impl Limoniter {
    /// The Assistant panel: every request made and what came of it, newest at the bottom, and the next request.
    pub(super) fn assistant_panel(&mut self, ui: &mut Ui) {
        if !self.assist_panel.open {
            return;
        }
        let mut open = true;
        let mut acts: Vec<PanelAct> = Vec::new();
        let range = (!self.no_file()).then(|| self.assist_range(self.assist_panel.task));
        let name = self.docs.get(self.active).map(Document::name).unwrap_or_default();
        let usable = !self.no_file() && !self.docs[self.active].hex_mode;
        let found = [Tool::Claude, Tool::Codex].map(|tool| installed(tool.program()));
        let working = self.assist_job.as_ref().map(|job| (job.exchange, job.started.elapsed().as_secs()));
        Panel::right("assistant-panel").default_size(380.0).size_range(260.0..=900.0).resizable(true).frame(egui::Frame::new().fill(theme::window()).inner_margin(egui::Margin::symmetric(6, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Assistant").color(theme::text()).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(Button::new(RichText::new("\u{D7}").color(theme::text_dim())).frame(false)).on_hover_text("Close").clicked() {
                        open = false;
                    }
                    if !self.assist_log.entries.is_empty() && ui.add(Button::new(RichText::new("Clear").color(theme::text_dim()).size(11.5)).frame(false)).on_hover_text("Forget the history").clicked() {
                        acts.push(PanelAct::Clear);
                    }
                });
            });
            ui.separator();
            // The next request, at the bottom.
            let panel = &mut self.assist_panel;
            let setup = &mut self.assistant;
            Panel::bottom("assistant-request").frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 6))).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for task in Task::ALL {
                        if ui.add(egui::Button::selectable(panel.task == task, task.label())).clicked() {
                            panel.task = task;
                            panel.error.clear();
                            panel.focus = true;
                        }
                    }
                });
                if let Some(range) = &range {
                    let place = if range.is_empty() { format!("the place after line {}", range.start) } else { format!("lines {} to {}", range.start + 1, range.end) };
                    ui.label(RichText::new(format!("Works on {place} of {name}: select some to choose.")).color(theme::text_dim()).size(11.5));
                }
                let hint = match panel.task {
                    Task::Fix => "Anything it should know, such as what goes wrong (optional)",
                    Task::CleanUp => "Anything it should keep or change (optional)",
                    Task::Write => "What the code should do",
                    Task::Explain => "Anything in particular to explain (optional)",
                    Task::Ask => "The question",
                };
                let field = ui.add(egui::TextEdit::multiline(&mut panel.words).hint_text(hint).desired_rows(3).desired_width(f32::INFINITY));
                if std::mem::take(&mut panel.focus) {
                    field.request_focus();
                }
                let enter = field.has_focus() && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift);
                ui.horizontal_wrapped(|ui| {
                    for (index, each) in [Tool::Claude, Tool::Codex].into_iter().enumerate() {
                        let label = if found[index] { each.name().to_string() } else { format!("{} (not installed)", each.name()) };
                        ui.radio_value(&mut setup.tool, each, label);
                    }
                    ui.checkbox(&mut panel.show_commands, "Commands");
                });
                if panel.show_commands {
                    egui::Grid::new("assistant-commands").num_columns(2).show(ui, |ui| {
                        ui.label("Claude Code");
                        ui.add(egui::TextEdit::singleline(&mut setup.claude).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                        ui.end_row();
                        ui.label("Codex");
                        ui.add(egui::TextEdit::singleline(&mut setup.codex).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                        ui.end_row();
                    });
                    if ui.small_button("Put the usual commands back").clicked() {
                        setup.claude = CLAUDE_COMMAND.into();
                        setup.codex = CODEX_COMMAND.into();
                    }
                }
                ui.label(RichText::new(format!("The part, and up to {CONTEXT_LINES} lines either side, go to {} (and so to {}), which can read this folder's files but not change them. Code that comes back is shown before anything changes.", setup.tool.name(), setup.tool.company())).color(theme::text_faint()).size(11.0));
                if !panel.error.is_empty() {
                    ui.label(RichText::new(panel.error.as_str()).color(theme::rainbow_mistake()));
                }
                ui.horizontal(|ui| {
                    let ready = usable && working.is_none();
                    let send = ui.add_enabled(ready, Button::new(RichText::new(format!("Send to {}", setup.tool.name())).strong())).on_hover_text("Or press Enter (Shift+Enter for a new line)").clicked();
                    if send || (enter && ready) {
                        acts.push(PanelAct::Send);
                    }
                });
            });
            // The history, newest at the bottom.
            egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                if self.assist_log.entries.is_empty() {
                    ui.add(egui::Label::new(RichText::new("Everything asked of Claude Code or Codex is kept here: what was asked, what came back, and what was done with it.").color(theme::text_dim())).wrap());
                }
                use crate::app::assistant_log::State;
                for entry in &self.assist_log.entries {
                    egui::Frame::group(ui.style()).inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(format!("{}  {} \u{B7} {}", entry.when, entry.tool.name(), entry.task.label())).color(theme::text_faint()).size(11.5));
                        if ui.add(egui::Label::new(RichText::new(entry.place()).color(theme::accent())).sense(egui::Sense::click())).on_hover_text("Go there").clicked() {
                            acts.push(PanelAct::GoTo(entry.id));
                        }
                        if !entry.words.is_empty() {
                            ui.add(egui::Label::new(RichText::new(format!("\u{201C}{}\u{201D}", entry.words)).italics()).wrap());
                        }
                        match entry.state {
                            State::Working => {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    let seconds = working.filter(|(id, _)| *id == entry.id).map_or(0, |(_, s)| s);
                                    ui.label(format!("Working\u{2026} {seconds}s"));
                                    if ui.small_button("Stop").clicked() {
                                        acts.push(PanelAct::Stop);
                                    }
                                });
                            }
                            State::Answered => {
                                ui.add(egui::Label::new(entry.reply.as_str()).selectable(true).wrap());
                            }
                            State::Suggested | State::PutIn | State::Left => {
                                ui.horizontal_wrapped(|ui| {
                                    let (added, removed) = entry.counts();
                                    let colour = match entry.state {
                                        State::PutIn => theme::change_saved(),
                                        State::Suggested => theme::accent(),
                                        _ => theme::text_dim(),
                                    };
                                    ui.label(RichText::new(format!("+{added} \u{2212}{removed} lines \u{B7} {}", entry.state.word())).color(colour));
                                    if ui.small_button("Show").on_hover_text("What it suggested, against what was there").clicked() {
                                        acts.push(PanelAct::Show(entry.id));
                                    }
                                });
                            }
                            State::Failed | State::Stopped => {
                                let words = if entry.state == State::Stopped { "Stopped before it answered.".to_string() } else { entry.reply.lines().take(6).collect::<Vec<_>>().join("\n") };
                                ui.add(egui::Label::new(RichText::new(words).color(theme::rainbow_mistake())).wrap());
                            }
                        }
                        if entry.seconds > 0 && entry.state != State::Working {
                            ui.label(RichText::new(format!("{}s", entry.seconds)).color(theme::text_faint()).size(10.5));
                        }
                    });
                }
            });
        });
        self.assist_panel.open = open;
        if working.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        for act in acts {
            match act {
                PanelAct::Send => {
                    let (task, words) = (self.assist_panel.task, self.assist_panel.words.trim_end().to_string());
                    match self.send_to_assistant(task, &words) {
                        Ok(()) => {
                            self.assist_panel.words.clear();
                            self.assist_panel.error.clear();
                        }
                        Err(problem) => self.assist_panel.error = problem,
                    }
                }
                PanelAct::Stop => self.stop_assistant(),
                PanelAct::Clear => {
                    if ask_yes_no("Assistant", "Forget the whole history of what's been asked?") {
                        self.assist_log.clear();
                    }
                }
                PanelAct::GoTo(id) => self.go_to_exchange(id),
                PanelAct::Show(id) => self.show_exchange(id),
            }
        }
    }

    /// The document an entry was about, if it's open, or opened from where it was.
    fn exchange_doc(&mut self, id: u64) -> Option<usize> {
        let entry = self.assist_log.entries.iter().find(|e| e.id == id)?.clone();
        if let Some(index) = entry.doc.and_then(|doc| self.docs.iter().position(|d| d.id == doc)) {
            return Some(index);
        }
        let path = entry.path.filter(|p| p.is_file())?;
        if let Some(index) = self.docs.iter().position(|d| d.path.as_deref() == Some(path.as_path())) {
            return Some(index);
        }
        self.open_path(&path).then_some(self.active)
    }

    /// Goes to the lines an entry was about.
    fn go_to_exchange(&mut self, id: u64) {
        let Some(index) = self.exchange_doc(id) else {
            return self.message("Assistant", "That file isn't open, and can't be found where it was.".into());
        };
        let start = self.assist_log.entries.iter().find(|e| e.id == id).map_or(0, |e| e.lines.start);
        self.active = index;
        let text = &mut self.doc().text;
        text.move_caret(start.min(text.line_count().saturating_sub(1)), 0, false);
        text.reveal = true;
        self.pending_focus = true;
    }

    /// Shows a suggestion again. One left out can still be put in, while the lines it was for are as they were.
    fn show_exchange(&mut self, id: u64) {
        let Some(entry) = self.assist_log.entries.iter().find(|e| e.id == id).cloned() else {
            return;
        };
        let index = self.exchange_doc(id);
        let (doc, text, range, done) = match index {
            Some(index) => {
                let now = self.docs[index].text.all_lines();
                let done = entry.state == crate::app::assistant_log::State::PutIn;
                match entry.text.clone() {
                    // While it's open, the part is found wherever it's moved to.
                    Some(text) if entry.doc == Some(self.docs[index].id) => (self.docs[index].id, text, entry.lines.clone(), done),
                    // From an earlier session, only if the lines are just as they were.
                    _ => {
                        let same = now.get(entry.lines.clone()).is_some_and(|lines| lines.iter().map(|l| l.as_ref()).eq(entry.old.iter().map(String::as_str)));
                        (self.docs[index].id, now, entry.lines.clone(), done || !same)
                    }
                }
            }
            None => (0, Vec::new(), entry.lines.clone(), true),
        };
        self.dialog = Dialog::AssistReview(Review { tool: entry.tool, doc, range, text, old: entry.old, new: entry.new, exchange: Some(id), done });
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
    fn every_request_and_what_came_of_it_is_kept_in_the_history() {
        use crate::app::assistant_log::State;
        let (dir, command) = stand_in("```js\nlet x = 2;\n```\n");
        let (mut app, ctx) = app_with_text(&["let x = 1;"]);
        app.assistant.claude = command;
        // Asked from the panel, as the person would.
        app.begin_assist(Task::Fix);
        assert!(app.assist_panel.open && app.assist_panel.task == Task::Fix);
        app.send_to_assistant(Task::Fix, "make it two").unwrap();
        assert_eq!(app.assist_log.entries.last().map(|e| e.state), Some(State::Working));
        wait(&mut app, &ctx);
        let entry = app.assist_log.entries.last().unwrap().clone();
        assert_eq!((entry.state, entry.words.as_str(), entry.new.clone()), (State::Suggested, "make it two", vec!["let x = 2;".to_string()]));
        assert_eq!(entry.place(), "line 1 of Edit1");
        // Left out, then looked at again from the history and put in after all.
        assert!(matches!(app.dialog, Dialog::AssistReview(_)), "the suggestion is shown first");
        app.close_dialog();
        app.assist_log.update(entry.id, |e| e.state = State::Left);
        app.show_exchange(entry.id);
        let Dialog::AssistReview(review) = std::mem::replace(&mut app.dialog, Dialog::None) else {
            panic!("shown again");
        };
        assert!(!review.done, "it can still be put in");
        app.accept_review(&review).unwrap();
        assert_eq!(app.docs[app.active].text.line(0).as_ref(), "let x = 2;");
        // An answer, a failure and a stop are kept too.
        app.assistant.claude = "echo not signed in 1>&2 & exit 3".into();
        app.send_to_assistant(Task::Explain, "").unwrap();
        wait(&mut app, &ctx);
        assert_eq!(app.assist_log.entries.last().map(|e| e.state), Some(State::Failed));
        assert!(app.assist_log.entries.last().unwrap().reply.contains("not signed in"));
        app.assistant.claude = if cfg!(windows) { "ping -n 30 127.0.0.1".into() } else { "sleep 30".into() };
        app.send_to_assistant(Task::Ask, "why?").unwrap();
        app.stop_assistant();
        assert_eq!(app.assist_log.entries.last().map(|e| e.state), Some(State::Stopped));
        assert_eq!(app.assist_log.entries.len(), 3);
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
