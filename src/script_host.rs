//! The window's side of macros and scripts: answering a running script, recording, the boxes they ask with, and the
//! buttons on the Advanced tab. How scripts themselves run is in `scripting.rs`.

use super::*;
use crate::scripting::{self, Answer, Call, Kind, LibraryAction, Next, Repeat, Search};

/// How long the window answers a script for in one frame, so that it goes on being drawn. A short macro finishes in less.
const SCRIPT_SLICE: Duration = Duration::from_millis(30);
/// A script that finishes quicker than this never shows that it was running.
const SHOW_RUNNING_AFTER: Duration = Duration::from_millis(400);
const MOST_OUTPUT_LINES: usize = 100_000;

/// What a command is called in a script, and in a recorded macro. Anything needing a box still asks when it's played.
pub(super) const SCRIPT_COMMANDS: [(&str, Command); 162] = [
    ("new_file", Command::New),
    ("open_file_box", Command::Open),
    ("save_file", Command::Save),
    ("save_as_box", Command::SaveAs),
    ("close_file", Command::CloseTab),
    ("undo", Command::Undo),
    ("redo", Command::Redo),
    ("cut", Command::Cut),
    ("copy", Command::Copy),
    ("paste", Command::Paste),
    ("select_all", Command::SelectAll),
    ("upper_case", Command::Upper),
    ("lower_case", Command::Lower),
    ("sentence_case", Command::SentenceCase),
    ("title_case", Command::TitleCase),
    ("toggle_hex", Command::ToggleHex),
    ("toggle_column_mode", Command::ToggleColumn),
    ("column_fill", Command::ColumnFill),
    ("column_cut", Command::ColumnCut),
    ("column_delete", Command::ColumnDelete),
    ("column_justify_left", Command::JustifyLeft),
    ("column_justify_center", Command::JustifyCenter),
    ("column_justify_right", Command::JustifyRight),
    ("csv_to_fixed", Command::CsvToFixed),
    ("fixed_to_csv", Command::FixedToCsv),
    ("column_number", Command::ColumnNumber),
    ("column_sum", Command::ColumnSum),
    ("print", Command::Print),
    ("last_change", Command::LastChange),
    ("next_change", Command::NextChange),
    ("split_horizontal", Command::SplitHorizontal),
    ("split_vertical", Command::SplitVertical),
    ("duplicate_window", Command::DuplicateWindow),
    ("filter_selection", Command::FilterSelection),
    ("collapse_all", Command::CollapseAll),
    ("expand_all", Command::ExpandAll),
    ("hide_lines", Command::HideLines),
    ("show_lines", Command::ShowLines),
    ("make_selections", Command::MakeSelections),
    ("compare", Command::Compare),
    ("select_inside_quotes", Command::TextObject(text_objects::Action::SelectInside, text_objects::Object::Quotes)),
    ("select_inside_brackets", Command::TextObject(text_objects::Action::SelectInside, text_objects::Object::Brackets)),
    ("select_inside_tag", Command::TextObject(text_objects::Action::SelectInside, text_objects::Object::Tag)),
    ("select_inside_word", Command::TextObject(text_objects::Action::SelectInside, text_objects::Object::Word)),
    ("select_inside_line", Command::TextObject(text_objects::Action::SelectInside, text_objects::Object::Line)),
    ("select_inside_paragraph", Command::TextObject(text_objects::Action::SelectInside, text_objects::Object::Paragraph)),
    ("select_around_quotes", Command::TextObject(text_objects::Action::SelectAround, text_objects::Object::Quotes)),
    ("select_around_brackets", Command::TextObject(text_objects::Action::SelectAround, text_objects::Object::Brackets)),
    ("select_around_tag", Command::TextObject(text_objects::Action::SelectAround, text_objects::Object::Tag)),
    ("select_around_word", Command::TextObject(text_objects::Action::SelectAround, text_objects::Object::Word)),
    ("select_around_line", Command::TextObject(text_objects::Action::SelectAround, text_objects::Object::Line)),
    ("select_around_paragraph", Command::TextObject(text_objects::Action::SelectAround, text_objects::Object::Paragraph)),
    ("replace_inside_quotes", Command::TextObject(text_objects::Action::ReplaceInside, text_objects::Object::Quotes)),
    ("replace_inside_brackets", Command::TextObject(text_objects::Action::ReplaceInside, text_objects::Object::Brackets)),
    ("replace_inside_tag", Command::TextObject(text_objects::Action::ReplaceInside, text_objects::Object::Tag)),
    ("replace_inside_word", Command::TextObject(text_objects::Action::ReplaceInside, text_objects::Object::Word)),
    ("replace_inside_line", Command::TextObject(text_objects::Action::ReplaceInside, text_objects::Object::Line)),
    ("replace_inside_paragraph", Command::TextObject(text_objects::Action::ReplaceInside, text_objects::Object::Paragraph)),
    ("delete_inside_quotes", Command::TextObject(text_objects::Action::DeleteInside, text_objects::Object::Quotes)),
    ("delete_inside_brackets", Command::TextObject(text_objects::Action::DeleteInside, text_objects::Object::Brackets)),
    ("delete_inside_tag", Command::TextObject(text_objects::Action::DeleteInside, text_objects::Object::Tag)),
    ("delete_inside_word", Command::TextObject(text_objects::Action::DeleteInside, text_objects::Object::Word)),
    ("delete_inside_line", Command::TextObject(text_objects::Action::DeleteInside, text_objects::Object::Line)),
    ("delete_inside_paragraph", Command::TextObject(text_objects::Action::DeleteInside, text_objects::Object::Paragraph)),
    ("delete_around_quotes", Command::TextObject(text_objects::Action::DeleteAround, text_objects::Object::Quotes)),
    ("delete_around_brackets", Command::TextObject(text_objects::Action::DeleteAround, text_objects::Object::Brackets)),
    ("delete_around_tag", Command::TextObject(text_objects::Action::DeleteAround, text_objects::Object::Tag)),
    ("delete_around_word", Command::TextObject(text_objects::Action::DeleteAround, text_objects::Object::Word)),
    ("delete_around_line", Command::TextObject(text_objects::Action::DeleteAround, text_objects::Object::Line)),
    ("delete_around_paragraph", Command::TextObject(text_objects::Action::DeleteAround, text_objects::Object::Paragraph)),
    ("narrow", Command::Narrow),
    ("widen", Command::Widen),
    ("toggle_preview", Command::TogglePreview),
    ("preview_off", Command::SetPreview(settings::PreviewView::Text)),
    ("preview_split", Command::SetPreview(settings::PreviewView::Split)),
    ("preview_full", Command::SetPreview(settings::PreviewView::Full)),
    ("reindent", Command::Reindent),
    ("wrap_to_line_ends", Command::WrapToLineEnds),
    ("line_ends_to_wrap", Command::LineEndsToWrap),
    ("paragraph_reformat", Command::ParagraphReformat),
    ("align_left", Command::AlignText(Align::Left)),
    ("align_center", Command::AlignText(Align::Center)),
    ("align_right", Command::AlignText(Align::Right)),
    ("align_justify", Command::AlignText(Align::Justify)),
    ("format_xml", Command::Format(formatters::Kind::Xml)),
    ("compress_xml", Command::Format(formatters::Kind::XmlCompact)),
    ("format_html", Command::Format(formatters::Kind::Html)),
    ("compress_html", Command::Format(formatters::Kind::HtmlCompact)),
    ("format_javascript", Command::Format(formatters::Kind::Js)),
    ("compress_javascript", Command::Format(formatters::Kind::JsCompact)),
    ("format_css", Command::Format(formatters::Kind::Css)),
    ("compress_css", Command::Format(formatters::Kind::CssCompact)),
    ("format_code_brace_same_line", Command::Format(formatters::Kind::CodeAttach)),
    ("format_code_brace_own_line", Command::Format(formatters::Kind::CodeBreak)),
    ("format_sql", Command::Format(formatters::Kind::Sql)),
    ("edi_to_lines", Command::Format(formatters::Kind::EdiLines)),
    ("edi_to_one_line", Command::Format(formatters::Kind::EdiOneLine)),
    ("format_json", Command::FormatJson),
    ("compress_json", Command::CompactJson),
    ("move_line_up", Command::MoveLineUp),
    ("move_line_down", Command::MoveLineDown),
    ("duplicate_line", Command::Duplicate),
    ("join_lines", Command::Join),
    ("sort_ascending", Command::SortAscending),
    ("sort_descending", Command::SortDescending),
    ("word_count", Command::WordCount),
    ("trim_leading", Command::TrimLeading),
    ("trim_trailing", Command::TrimTrailing),
    ("tabs_to_spaces", Command::TabsToSpaces),
    ("spaces_to_tabs", Command::SpacesToTabs),
    ("indent", Command::Indent),
    ("outdent", Command::Outdent),
    ("go_to_brace", Command::GoToBrace),
    ("select_to_brace", Command::SelectToBrace),
    ("compute_hash", Command::ComputeHash),
    ("generate_guid", Command::GenerateGuid),
    ("convert_number", Command::ConvertNumber),
    ("copy_path", Command::CopyPath),
    ("toggle_bookmark", Command::ToggleBookmark),
    ("next_bookmark", Command::NextBookmark),
    ("previous_bookmark", Command::PreviousBookmark),
    ("previous_tab", Command::PreviousTab),
    ("next_tab", Command::NextTab),
    ("comment", Command::Comment),
    ("uncomment", Command::Uncomment),
    ("go_back", Command::NavBack),
    ("go_forward", Command::NavForward),
    ("toggle_word_wrap", Command::ToggleWrap),
    ("toggle_read_only", Command::ToggleReadOnly),
    ("insert_date_time", Command::InsertDateTime),
    ("insert_date", Command::InsertDate),
    ("insert_time", Command::InsertTime),
    ("insert_iso_date", Command::InsertIsoDate),
    ("insert_file_name", Command::InsertFileName),
    ("insert_file_path", Command::InsertFilePath),
    ("insert_page_break", Command::InsertPageBreak),
    ("insert_file", Command::InsertFile),
    ("view_text", Command::ViewRegular),
    ("view_hex", Command::ViewHex),
    ("view_ebcdic", Command::ViewEbcdic),
    ("hex_copy_selected", Command::HexCopySelected),
    ("swap_with_clipboard", Command::SwapClipboard),
    ("last_used_file", Command::UsedFileBack),
    ("least_recently_used_file", Command::UsedFileForward),
    ("save_selection_box", Command::SaveSelection),
    ("split_file_box", Command::SplitFile),
    ("rename_file_box", Command::RenameFile),
    ("show_in_explorer", Command::ShowInExplorer),
    ("open_in_usual_program", Command::OpenWithDefault),
    ("snippets_box", Command::Snippets),
    ("run_command_box", Command::RunTool),
    ("next_spelling_mistake", Command::NextMistake),
    ("history_of_this_part", Command::PartHistory),
    ("tag_red", Command::Tag(0)),
    ("tag_orange", Command::Tag(1)),
    ("tag_yellow", Command::Tag(2)),
    ("tag_green", Command::Tag(3)),
    ("tag_blue", Command::Tag(4)),
    ("tag_purple", Command::Tag(5)),
    ("remove_tag", Command::Untag),
    ("next_tag", Command::NextTag),
    ("previous_tag", Command::PreviousTag),
];

/// What a running script is waiting for, beyond the file being free.
pub(super) enum Waiting {
    /// The person answering a box.
    Box,
    /// A search of a big file, which takes many frames. Holds where the search began.
    Search { caret: (usize, usize), anchor: (usize, usize), backward: bool },
    /// A Replace All on a big file.
    Replace,
}

pub(super) struct ScriptRun {
    run: scripting::Run,
    waiting: Option<Waiting>,
    printed: bool,
    /// What to do once it has finished, as saving after an add-on's on_save.
    after: Option<After>,
}

/// Something to do when a script finishes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum After {
    /// Save this document, as Save was asked for before the add-ons ran.
    Save(u64),
    /// Send this document to the device chosen, once the add-ons' on_send functions have run.
    Send(u64),
    /// Run this document where it was asked to run, once the add-ons' on_run functions have run.
    Run(u64),
}

/// The macro Play macro plays. It's kept on disk once it's been given a name.
pub(super) struct CurrentMacro {
    pub name: String,
    pub path: Option<PathBuf>,
    /// Only for one that hasn't been saved. A saved one is read when played, so edits to it count.
    pub source: String,
}

/// The boxes macros and scripts need.
impl ScriptBox {
    pub(super) fn new_addon() -> Self {
        Self::NewAddOn { name: "My add-on".into(), error: String::new(), focus: true }
    }
}

pub(super) enum ScriptBox {
    SaveMacro { source: String, name: String, error: String, focus: bool },
    PlayTimes { times: String, until_end: bool, error: String, focus: bool, path: Option<PathBuf> },
    NewScript { name: String, error: String, focus: bool },
    NewAddOn { name: String, error: String, focus: bool },
    Rename { path: PathBuf, name: String, error: String, focus: bool },
    Delete { path: PathBuf, name: String },
    Ask { title: String, prompt: String, value: String, focus: bool },
    Confirm { title: String, question: String },
}

/// What to do once a box has been drawn, away from the borrow of the box.
enum BoxOutcome {
    Nothing,
    Close,
    SaveMacro(String, String),
    Play(u64, bool, Option<PathBuf>),
    TimesError(String),
    NewScript(String),
    NewAddOn(String),
    Rename(PathBuf, String),
    Delete(PathBuf),
    Answer(Answer),
}

fn name_of_path(path: &Path) -> String {
    path.file_stem().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

fn set_box_error(dialog: &mut Dialog, message: String) {
    if let Dialog::Script(ScriptBox::SaveMacro { error, .. } | ScriptBox::NewScript { error, .. } | ScriptBox::NewAddOn { error, .. } | ScriptBox::Rename { error, .. } | ScriptBox::PlayTimes { error, .. }) = dialog {
        *error = message;
    }
}

fn named_field(ui: &mut Ui, value: &mut String, focus: &mut bool) -> bool {
    let field = ui.text_edit_singleline(value);
    if *focus {
        field.request_focus();
        *focus = false;
    }
    field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
}

fn error_line(ui: &mut Ui, error: &str) {
    if !error.is_empty() {
        ui.colored_label(theme::ruler_mark(), error);
    }
}

fn script_box(title: &str, ctx: &Context, content: impl FnOnce(&mut Ui)) {
    egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, content);
}

impl Limoniter {
    // ---------- Starting and finishing ----------

    /// Plays some source, whatever it came from. At most one runs at a time.
    pub(super) fn play_source(&mut self, name: &str, source: &str, repeat: Repeat, ui: &Ui) {
        if self.script.is_some() {
            return;
        }
        if scripting::recording() {
            self.message("Recording", "Stop recording first. A macro can't play while another is being recorded.".into());
            return;
        }
        match scripting::start(name, source, repeat) {
            Ok(run) => {
                self.script = Some(Box::new(ScriptRun { run, waiting: None, printed: false, after: None }));
                self.continue_script(ui);
            }
            Err(problem) => self.message(name, problem),
        }
    }

    /// Runs one function of an add-on, then whatever is to follow it. If it can't start, what follows still happens.
    pub(super) fn play_function(&mut self, name: &str, path: &Path, function: &str, after: Option<After>, ui: &Ui) {
        if self.script.is_some() {
            return;
        }
        let started = std::fs::read_to_string(path).map_err(|e| format!("{} couldn't be read.\n\n{e}", path.display())).and_then(|source| scripting::start_function(name, &source, function));
        match started {
            Ok(run) => {
                self.script = Some(Box::new(ScriptRun { run, waiting: None, printed: false, after }));
                self.continue_script(ui);
            }
            Err(problem) => {
                self.message(name, problem);
                if let Some(after) = after {
                    self.do_after(after);
                }
            }
        }
    }

    fn do_after(&mut self, after: After) {
        match after {
            After::Save(doc_id) => {
                if let Some(index) = self.docs.iter().position(|doc| doc.id == doc_id) {
                    self.save(index);
                }
            }
            After::Send(doc_id) => self.finish_send(doc_id),
            After::Run(doc_id) => self.finish_run(doc_id),
        }
    }

    /// Reads a macro or script and plays it. If it's open in a tab, that's what plays, unsaved edits and all.
    pub(super) fn play_path(&mut self, path: &Path, repeat: Repeat, ui: &Ui) {
        let name = name_of_path(path);
        let open = self.docs.iter().position(|doc| doc.path.as_deref().is_some_and(|known| same_path(known, path)));
        if open == Some(self.active) && !self.no_file() {
            self.message(&name, format!("{name} is the file in front, and would be working on itself. Go to the file you want it to work on, then play it from there."));
            return;
        }
        let source = match open {
            Some(index) if !self.docs[index].text.is_disk_backed() && !self.docs[index].hex_mode => self.docs[index].text.all_lines().iter().map(|line| line.to_string()).collect::<Vec<_>>().join("\n"),
            _ => match std::fs::read_to_string(path) {
                Ok(source) => source,
                Err(e) => {
                    self.message(&name, format!("{} couldn't be read.\n\n{e}", path.display()));
                    return;
                }
            },
        };
        self.play_source(&name, &source, repeat, ui);
    }

    pub(super) fn play_current_macro(&mut self, repeat: Repeat, ui: &Ui) {
        match &self.current_macro {
            Some(CurrentMacro { path: Some(path), .. }) => {
                let path = path.clone();
                self.play_path(&path, repeat, ui);
            }
            Some(CurrentMacro { name, source, .. }) => {
                let (name, source) = (name.clone(), source.clone());
                self.play_source(&name, &source, repeat, ui);
            }
            None => self.message("Play macro", "There's no macro to play yet. Record one from Tools, Macros, or load one.".into()),
        }
    }

    fn finish_script(&mut self, mut run: ScriptRun, result: Result<(), String>) {
        let mut changed = false;
        for doc in &mut self.docs {
            changed |= doc.text.end_undo_group();
        }
        self.pending_focus = true;
        run.waiting = None;
        if let Err(problem) = result {
            let undo = if changed { "\n\nWhat it changed before that can be taken back with one Undo." } else { "" };
            self.message(&run.run.name, format!("{} stopped.\n\n{problem}{undo}", run.run.name));
        }
        if let Some(after) = run.after.take() {
            self.do_after(after);
        }
    }

    /// Stops the running script straight away. Its thread sees that at its next step.
    pub(super) fn stop_script(&mut self) {
        let Some(run) = self.script.take() else {
            return;
        };
        if matches!(run.waiting, Some(Waiting::Search { .. } | Waiting::Replace)) {
            self.pending = None;
            self.find.status.clear();
        }
        let name = run.run.name.clone();
        drop(run);
        let mut changed = false;
        for doc in &mut self.docs {
            changed |= doc.text.end_undo_group();
        }
        self.pending_focus = true;
        if changed {
            self.message(&name, format!("{name} was stopped. One Undo takes back what it had done."));
        }
    }

    // ---------- Answering a running script ----------

    /// Answers the script for a while. It waits its turn for a file that's busy and for any box that's open.
    pub(super) fn continue_script(&mut self, ui: &Ui) {
        let Some(mut run) = self.script.take() else {
            return;
        };
        let began = std::time::Instant::now();
        let mut finished = None;
        while finished.is_none() {
            if !matches!(self.dialog, Dialog::None) || self.pending.is_some() || self.docs[self.active].is_busy() {
                break;
            }
            if let Some(waiting) = run.waiting.take() {
                run.run.answer(self.waiting_over(waiting));
            }
            let left = SCRIPT_SLICE.saturating_sub(began.elapsed());
            match run.run.next(left) {
                Next::Call(call) => {
                    // The clipboard only changes when the frame ends, so whatever's next has to wait for that.
                    let end_frame = matches!(&call, Call::SetClipboard(_)) || matches!(&call, Call::Command(name) if matches!(name.as_str(), "cut" | "copy" | "hex_copy_selected"));
                    self.docs[self.active].text.begin_undo_group(run.run.keep_steps);
                    if let Some(reply) = self.script_call(call, &mut run, ui) {
                        run.run.answer(reply);
                    }
                    if end_frame {
                        break;
                    }
                }
                Next::Finished(result) => finished = Some(result),
                Next::Nothing => break,
            }
            if began.elapsed() >= SCRIPT_SLICE {
                break;
            }
        }
        match finished {
            Some(result) => self.finish_script(*run, result),
            None => {
                self.script = Some(run);
                ui.ctx().request_repaint();
            }
        }
    }

    /// The answer to something that has taken a while.
    fn waiting_over(&mut self, waiting: Waiting) -> Result<Answer, String> {
        match waiting {
            Waiting::Box => Ok(self.script_answer.take().unwrap_or(Answer::Nothing)),
            Waiting::Replace => Ok(Answer::Number(self.pending_count as i64)),
            Waiting::Search { caret, anchor, backward } => {
                let found = self.pending_count > 0;
                Ok(Answer::Yes(found && self.keep_if_not_wrapped(caret, anchor, backward)))
            }
        }
    }

    /// A search in a script stops at the end of the file, so that a loop over the matches comes to an end. The search
    /// itself goes round to the top, as Find Next does, so one that's come round is undone here.
    fn keep_if_not_wrapped(&mut self, caret: (usize, usize), anchor: (usize, usize), backward: bool) -> bool {
        let text = &mut self.docs[self.active].text;
        let (old_start, old_end) = if caret < anchor { (caret, anchor) } else { (anchor, caret) };
        let (now_caret, now_anchor) = ((text.caret_line, text.caret_col), (text.anchor_line, text.anchor_col));
        let (new_start, new_end) = if now_caret < now_anchor { (now_caret, now_anchor) } else { (now_anchor, now_caret) };
        let onward = if backward { new_end <= old_start && (new_start, new_end) != (old_start, old_end) } else { new_start >= old_end && (new_start, new_end) != (old_start, old_end) };
        if !onward {
            text.set_selection(anchor, caret, false);
        }
        onward
    }

    fn script_search(&self, find: &str, replace: &str, search: Search) -> SearchOptions {
        let format = &self.docs[self.active].text.format;
        let by_bytes = format.charset.keep_bytes;
        SearchOptions {
            pattern: if by_bytes { format.prepare_input(find, format.state) } else { find.to_string() },
            replacement: if by_bytes { format.prepare_input(replace, format.state) } else { replace.to_string() },
            match_case: search.match_case,
            whole_word: !by_bytes && search.whole_word,
            regex: !by_bytes && search.regex,
            hex_bytes: false,
            backward: search.backward,
        }
    }

    /// Does what a script asks, or returns None if the answer has to wait.
    fn script_call(&mut self, call: Call, run: &mut ScriptRun, ui: &Ui) -> Option<Result<Answer, String>> {
        if scripting::is_about_text(&call) {
            if self.no_file() {
                return Some(Err("No file is open. new_file() or open_file() can give it one.".into()));
            }
            let doc = &self.docs[self.active];
            if doc.hex_mode {
                return Some(Err("This file is showing in hex mode. A script needs it in text mode for this.".into()));
            }
            if scripting::edits_text(&call) && doc.text.read_only {
                return Some(Err("This file is locked against changes, so it can't be changed.".into()));
            }
        }
        let needs_file = matches!(call, Call::Save | Call::SaveAs(_) | Call::FileName | Call::FilePath | Call::IsModified);
        if needs_file && self.no_file() {
            return Some(Err("No file is open.".into()));
        }
        let name = run.run.name.clone();
        Some(match call {
            Call::Find(find, search) => {
                let options = self.script_search(&find, "", search);
                let text = &self.docs[self.active].text;
                let (caret, anchor) = ((text.caret_line, text.caret_col), (text.anchor_line, text.anchor_col));
                if text.is_disk_backed() {
                    return match text.begin_find(&options) {
                        Ok(Some(cursor)) => {
                            self.pending = Some(Pending::Find(cursor));
                            self.pending_share = 0.0;
                            self.pending_count = 0;
                            run.waiting = Some(Waiting::Search { caret, anchor, backward: search.backward });
                            None
                        }
                        Ok(None) => Some(Ok(Answer::Yes(false))),
                        Err(e) => Some(Err(e)),
                    };
                }
                match self.docs[self.active].text.find(&options) {
                    Ok(found) => Ok(Answer::Yes(found && self.keep_if_not_wrapped(caret, anchor, search.backward))),
                    Err(e) => Err(e),
                }
            }
            Call::ReplaceNext(find, with, search) => {
                let options = self.script_search(&find, &with, search);
                self.docs[self.active].text.replace_current(&options).map(Answer::Yes)
            }
            Call::ReplaceAll(find, with, search) => {
                let options = self.script_search(&find, &with, search);
                let text = &mut self.docs[self.active].text;
                if text.is_disk_backed() {
                    return match text.begin_replace_all(&options) {
                        Ok(Some(cursor)) => {
                            self.pending = Some(Pending::ReplaceAll(cursor));
                            self.pending_share = 0.0;
                            self.pending_count = 0;
                            run.waiting = Some(Waiting::Replace);
                            None
                        }
                        Ok(None) => Some(Ok(Answer::Number(0))),
                        Err(e) => Some(Err(e)),
                    };
                }
                text.replace_all(&options).map(|count| Answer::Number(count as i64))
            }
            Call::Command(wanted) => match SCRIPT_COMMANDS.iter().find(|(known, _)| *known == wanted) {
                Some((_, command)) => {
                    self.run(*command, ui);
                    Ok(Answer::Nothing)
                }
                None => Err(format!("There's no command called {wanted}.")),
            },
            Call::Message(text) => {
                self.message(&name, text);
                Ok(Answer::Nothing)
            }
            Call::Ask(prompt, value) => {
                self.dialog = Dialog::Script(ScriptBox::Ask { title: name, prompt, value, focus: true });
                run.waiting = Some(Waiting::Box);
                return None;
            }
            Call::Confirm(question) => {
                self.dialog = Dialog::Script(ScriptBox::Confirm { title: name, question });
                run.waiting = Some(Waiting::Box);
                return None;
            }
            Call::Print(text) => {
                self.script_print(text, !run.printed);
                run.printed = true;
                Ok(Answer::Nothing)
            }
            Call::NewFile => {
                self.new_document();
                Ok(Answer::Nothing)
            }
            Call::OpenFile(path) => {
                let path = self.place_for_script(&path);
                Ok(Answer::Yes(path.is_file() && self.open_path(&path)))
            }
            Call::Save => {
                if self.docs[self.active].path.is_none() {
                    Err("This file has no name yet. Use save_as to give it one.".to_string())
                } else {
                    let active = self.active;
                    Ok(Answer::Yes(self.save(active)))
                }
            }
            Call::SaveAs(path) => {
                let path = self.place_for_script(&path);
                if path.exists() {
                    Err(format!("{} is there already. A script can't write over another file.", path.display()))
                } else {
                    let active = self.active;
                    Ok(Answer::Yes(self.write_to(active, &path)))
                }
            }
            Call::NextFile => {
                self.step_tab(true);
                Ok(Answer::Nothing)
            }
            Call::PreviousFile => {
                self.step_tab(false);
                Ok(Answer::Nothing)
            }
            Call::FileCount => Ok(Answer::Number(self.docs.iter().filter(|doc| !doc.placeholder).count() as i64)),
            Call::FileName => Ok(Answer::Text(self.docs[self.active].name())),
            Call::FilePath => Ok(Answer::Text(self.docs[self.active].path.as_ref().map_or_else(String::new, |path| path.display().to_string()))),
            Call::IsModified => Ok(Answer::Yes(self.docs[self.active].is_modified())),
            Call::Clipboard => Ok(Answer::Text(arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()).unwrap_or_default())),
            Call::SetClipboard(text) => {
                ui.ctx().copy_text(text);
                Ok(Answer::Nothing)
            }
            Call::Panel(title, text, add) => {
                self.show_addon_panel(title, &text, add);
                Ok(Answer::Nothing)
            }
            Call::SendDevice => Ok(Answer::Text(self.send_pending.as_ref().map_or_else(String::new, |pending| pending.device.name.clone()))),
            Call::SetSendName(name) => match (&mut self.send_pending, crate::deploy::name_problem(&name)) {
                (None, _) => Err("set_send_name only works in an add-on's on_send function.".into()),
                (Some(_), Some(problem)) => Err(problem),
                (Some(pending), None) => {
                    pending.name = name.trim().to_string();
                    Ok(Answer::Nothing)
                }
            },
            other => {
                let doc = &mut self.docs[self.active];
                scripting::on_text(&mut doc.text, &mut doc.text_state, other)
            }
        })
    }

    /// A path in a script means somewhere beside the file in front, unless it says where.
    fn place_for_script(&self, path: &str) -> PathBuf {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            return path;
        }
        match self.docs[self.active].path.as_deref().and_then(Path::parent) {
            Some(folder) => folder.join(path),
            None => path,
        }
    }

    /// One line in the Output window. The first of a run replaces whatever was showing there.
    pub(super) fn script_print(&mut self, text: String, first: bool) {
        if first {
            if let Some(job) = self.job.take() {
                job.cancel.store(true, Ordering::Relaxed);
            }
            self.replace_plan = None;
            self.script_output.clear();
        }
        self.show_results = true;
        self.script_output.extend(text.lines().map(str::to_string));
        if text.is_empty() {
            self.script_output.push(String::new());
        }
        if self.script_output.len() > MOST_OUTPUT_LINES {
            let extra = self.script_output.len() - MOST_OUTPUT_LINES;
            self.script_output.drain(0..extra);
        }
    }

    // ---------- Recording ----------

    fn toggle_recording(&mut self) {
        if !scripting::recording() {
            if self.docs[self.active].hex_mode {
                self.message("Record", "Macros record typing in text mode. Switch to text mode first.".into());
                return;
            }
            scripting::start_recording();
            self.pending_focus = true;
            return;
        }
        let Some(source) = scripting::stop_recording() else {
            self.message("Record", "Nothing was recorded, so there's no new macro.".into());
            return;
        };
        self.library.refresh();
        let mut number = 1;
        while self.library.macros.iter().any(|item| item.name.eq_ignore_ascii_case(&format!("Macro {number}"))) {
            number += 1;
        }
        self.current_macro = Some(CurrentMacro { name: format!("Macro {number}"), path: None, source: source.clone() });
        self.dialog = Dialog::Script(ScriptBox::SaveMacro { source, name: format!("Macro {number}"), error: String::new(), focus: true });
    }

    /// Notes a search for the macro being recorded, with its words written out so that it needs nothing else to play.
    pub(super) fn record_find(&self, backward: bool) {
        if scripting::recording() && !self.find.find.is_empty() {
            let options = scripting::options_text(self.recorded_search(backward));
            scripting::note_line(format!("find({}{options});", scripting::quoted(&self.find.find)));
        }
    }

    pub(super) fn record_replace(&self, function: &str) {
        if scripting::recording() && !self.find.find.is_empty() {
            let options = scripting::options_text(self.recorded_search(false));
            scripting::note_line(format!("{function}({}, {}{options});", scripting::quoted(&self.find.find), scripting::quoted(&self.find.replace)));
        }
    }

    fn recorded_search(&self, backward: bool) -> Search {
        Search { match_case: self.find.match_case, whole_word: self.find.whole_word, regex: self.find.regex, backward }
    }

    pub(super) fn record_command(&self, command: Command) {
        if scripting::recording()
            && let Some((name, _)) = SCRIPT_COMMANDS.iter().find(|(_, known)| *known == command)
        {
            scripting::note_line(format!("command(\"{name}\");"));
        }
    }

    // ---------- Commands ----------

    pub(super) fn script_command(&mut self, command: Command, ui: &Ui) {
        match command {
            Command::RecordMacro => self.toggle_recording(),
            Command::PlayMacro => self.play_current_macro(Repeat::Once, ui),
            Command::PlayMacroTimes => {
                if self.current_macro.is_none() {
                    self.play_current_macro(Repeat::Once, ui);
                } else {
                    self.ask_how_many_times(None);
                }
            }
            Command::LoadMacro => {
                if let Some(path) = rfd::FileDialog::new().set_title("Load macro").add_filter("Limoniter macro", &[scripting::EXTENSION]).pick_file() {
                    self.load_into_library(Kind::Macro, &path);
                }
            }
            Command::LoadScript => {
                if let Some(path) = rfd::FileDialog::new().set_title("Load script").add_filter("Limoniter script", &[scripting::EXTENSION]).pick_file() {
                    self.load_into_library(Kind::Script, &path);
                }
            }
            Command::NewScript => self.dialog = Dialog::Script(ScriptBox::NewScript { name: "My script".into(), error: String::new(), focus: true }),
            Command::ShowMacros => {
                self.panels.macros = !self.panels.macros;
                self.library.refresh();
            }
            Command::ShowScripts => {
                self.panels.scripts = !self.panels.scripts;
                self.library.refresh();
            }
            _ => {}
        }
    }

    fn ask_how_many_times(&mut self, path: Option<PathBuf>) {
        self.dialog = Dialog::Script(ScriptBox::PlayTimes { times: "2".into(), until_end: false, error: String::new(), focus: true, path });
    }

    fn load_into_library(&mut self, kind: Kind, from: &Path) {
        match self.library.add_copy(kind, from) {
            Ok(path) => {
                self.library.refresh();
                if kind == Kind::Macro {
                    self.current_macro = Some(CurrentMacro { name: name_of_path(&path), path: Some(path), source: String::new() });
                    self.panels.macros = true;
                } else {
                    self.panels.scripts = true;
                }
            }
            Err(problem) => self.message("Load", problem),
        }
    }

    pub(super) fn library_action(&mut self, action: LibraryAction, ui: &Ui) {
        match action {
            LibraryAction::Play(path) => {
                if self.library.macros.iter().any(|item| item.path == path) {
                    self.current_macro = Some(CurrentMacro { name: name_of_path(&path), path: Some(path.clone()), source: String::new() });
                }
                self.play_path(&path, Repeat::Once, ui);
            }
            LibraryAction::PlayTimes(path) => {
                if self.library.macros.iter().any(|item| item.path == path) {
                    self.current_macro = Some(CurrentMacro { name: name_of_path(&path), path: Some(path.clone()), source: String::new() });
                }
                self.ask_how_many_times(Some(path));
            }
            LibraryAction::Edit(path) => {
                self.open_path(&path);
            }
            LibraryAction::Rename(path) => self.dialog = Dialog::Script(ScriptBox::Rename { name: name_of_path(&path), path, error: String::new(), focus: true }),
            LibraryAction::Delete(path) => self.dialog = Dialog::Script(ScriptBox::Delete { name: name_of_path(&path), path }),
            LibraryAction::SetKey(path, digit) => self.library.set_key(&path, digit),
            LibraryAction::Record => self.toggle_recording(),
            LibraryAction::New => self.dialog = Dialog::Script(ScriptBox::NewScript { name: "My script".into(), error: String::new(), focus: true }),
            LibraryAction::LoadMacro => self.run(Command::LoadMacro, ui),
            LibraryAction::LoadScript => self.run(Command::LoadScript, ui),
        }
    }

    /// Plays whatever has been given a key, if its key was pressed.
    pub(super) fn script_shortcuts(&mut self, ui: &Ui) {
        const DIGITS: [Key; 10] = [Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
        let keys = self.library.keys.clone();
        for (digit, path) in keys {
            let shortcut = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::ALT, DIGITS[usize::from(digit) % 10]);
            if ui.ctx().input_mut(|i| i.consume_shortcut(&shortcut)) {
                self.play_path(&path, Repeat::Once, ui);
            }
        }
    }

    // ---------- Boxes ----------

    /// Draws a box of ours if one's open, and the Stop button of a script that's taking a while.
    pub(super) fn script_dialog(&mut self, ctx: &Context) -> bool {
        self.running_window(ctx);
        let Dialog::Script(state) = &mut self.dialog else {
            return false;
        };
        let cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut outcome = BoxOutcome::Nothing;
        match state {
            ScriptBox::SaveMacro { source, name, error, focus } => script_box("Save macro", ctx, |ui| {
                ui.label("Give it a name to keep it. Without one it stays until you record another or close Limoniter.");
                let enter = named_field(ui, name, focus);
                error_line(ui, error);
                ui.add_space(6.0);
                ui.collapsing("What was recorded", |ui| {
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| ui.label(RichText::new(source.as_str()).monospace().size(12.0)));
                });
                ui.add_space(6.0);
                let (save, later) = ui.horizontal(|ui| (ui.button("Save").clicked(), ui.button("Don't save").clicked())).inner;
                if save || enter {
                    outcome = BoxOutcome::SaveMacro(name.clone(), source.clone());
                } else if later {
                    outcome = BoxOutcome::Close;
                }
            }),
            ScriptBox::PlayTimes { times, until_end, error, focus, path } => script_box("Play several times", ctx, |ui| {
                ui.add_enabled_ui(!*until_end, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Times:");
                        let field = ui.add(egui::TextEdit::singleline(times).desired_width(80.0));
                        if *focus {
                            field.request_focus();
                            *focus = false;
                        }
                    });
                });
                ui.checkbox(until_end, "Keep going until the end of the file").on_hover_text("It plays until a run doesn't move the cursor down a line, and doesn't remove any lines");
                error_line(ui, error);
                let (ok, cancel) = ok_cancel(ui);
                if ok || ui.input(|i| i.key_pressed(Key::Enter)) {
                    outcome = match times.trim().parse::<u64>() {
                        _ if *until_end => BoxOutcome::Play(0, true, path.clone()),
                        Ok(count) if count >= 1 => BoxOutcome::Play(count, false, path.clone()),
                        _ => BoxOutcome::TimesError("Type a number of times, from 1 up.".into()),
                    };
                } else if cancel {
                    outcome = BoxOutcome::Close;
                }
            }),
            ScriptBox::NewScript { name, error, focus } => script_box("New script", ctx, |ui| {
                ui.label("Name:");
                let enter = named_field(ui, name, focus);
                error_line(ui, error);
                let (ok, cancel) = ok_cancel(ui);
                if ok || enter {
                    outcome = BoxOutcome::NewScript(name.clone());
                } else if cancel {
                    outcome = BoxOutcome::Close;
                }
            }),
            ScriptBox::NewAddOn { name, error, focus } => script_box("New add-on", ctx, |ui| {
                ui.label("Name:");
                let enter = named_field(ui, name, focus);
                error_line(ui, error);
                let (ok, cancel) = ok_cancel(ui);
                if ok || enter {
                    outcome = BoxOutcome::NewAddOn(name.clone());
                } else if cancel {
                    outcome = BoxOutcome::Close;
                }
            }),
            ScriptBox::Rename { path, name, error, focus } => script_box("Rename", ctx, |ui| {
                ui.label("New name:");
                let enter = named_field(ui, name, focus);
                error_line(ui, error);
                let (ok, cancel) = ok_cancel(ui);
                if ok || enter {
                    outcome = BoxOutcome::Rename(path.clone(), name.clone());
                } else if cancel {
                    outcome = BoxOutcome::Close;
                }
            }),
            ScriptBox::Delete { path, name } => script_box("Delete", ctx, |ui| {
                ui.label(format!("Delete {name}? This can't be undone."));
                ui.add_space(6.0);
                let (delete, cancel) = ui.horizontal(|ui| (ui.button("Delete").clicked(), ui.button("Cancel").clicked())).inner;
                if delete {
                    outcome = BoxOutcome::Delete(path.clone());
                } else if cancel {
                    outcome = BoxOutcome::Close;
                }
            }),
            ScriptBox::Ask { title, prompt, value, focus } => script_box(title, ctx, |ui| {
                ui.label(prompt.as_str());
                let enter = named_field(ui, value, focus);
                let (ok, cancel) = ok_cancel(ui);
                if ok || enter {
                    outcome = BoxOutcome::Answer(Answer::Text(value.clone()));
                } else if cancel || cancelled {
                    outcome = BoxOutcome::Answer(Answer::Text(String::new()));
                }
            }),
            ScriptBox::Confirm { title, question } => script_box(title, ctx, |ui| {
                ui.label(question.as_str());
                ui.add_space(6.0);
                let (yes, no) = ui.horizontal(|ui| (ui.button("Yes").clicked(), ui.button("No").clicked())).inner;
                if yes {
                    outcome = BoxOutcome::Answer(Answer::Yes(true));
                } else if no || cancelled {
                    outcome = BoxOutcome::Answer(Answer::Yes(false));
                }
            }),
        }
        if cancelled && matches!(outcome, BoxOutcome::Nothing) {
            outcome = BoxOutcome::Close;
        }
        self.box_outcome(outcome, ctx);
        true
    }

    fn box_outcome(&mut self, outcome: BoxOutcome, ctx: &Context) {
        match outcome {
            BoxOutcome::Nothing => {}
            BoxOutcome::Close => self.close_dialog(),
            BoxOutcome::Answer(answer) => {
                self.script_answer = Some(answer);
                self.close_dialog();
            }
            BoxOutcome::TimesError(problem) => set_box_error(&mut self.dialog, problem),
            BoxOutcome::SaveMacro(name, source) => match self.library.save(Kind::Macro, &name, &source, false) {
                Ok(path) => {
                    self.current_macro = Some(CurrentMacro { name: name_of_path(&path), path: Some(path), source: String::new() });
                    self.close_dialog();
                }
                Err(problem) => set_box_error(&mut self.dialog, problem),
            },
            BoxOutcome::NewScript(name) => match self.library.save(Kind::Script, &name, scripting::NEW_SCRIPT, false) {
                Ok(path) => {
                    self.close_dialog();
                    self.panels.scripts = true;
                    self.open_path(&path);
                }
                Err(problem) => set_box_error(&mut self.dialog, problem),
            },
            BoxOutcome::NewAddOn(name) => match self.create_addon(&name) {
                Ok(()) => self.close_dialog(),
                Err(problem) => set_box_error(&mut self.dialog, problem),
            },
            BoxOutcome::Rename(path, name) => match self.library.rename(&path, &name) {
                Ok(target) => {
                    self.close_dialog();
                    if let Some(current) = &mut self.current_macro
                        && current.path.as_deref() == Some(path.as_path())
                    {
                        current.name = name_of_path(&target);
                        current.path = Some(target.clone());
                    }
                    for doc in &mut self.docs {
                        if doc.path.as_deref().is_some_and(|known| same_path(known, &path)) {
                            doc.path = Some(target.clone());
                        }
                    }
                }
                Err(problem) => set_box_error(&mut self.dialog, problem),
            },
            BoxOutcome::Delete(path) => {
                self.close_dialog();
                if let Err(problem) = self.library.delete(&path) {
                    self.message("Delete", problem);
                } else if self.current_macro.as_ref().is_some_and(|current| current.path.as_deref() == Some(path.as_path())) {
                    self.current_macro = None;
                }
            }
            BoxOutcome::Play(times, until_end, path) => {
                self.close_dialog();
                let repeat = if until_end { Repeat::UntilEnd } else { Repeat::Times(times) };
                self.pending_play = Some((path, repeat));
                ctx.request_repaint();
            }
        }
    }

    /// As if OK had been pressed on whichever box is open.
    #[cfg(test)]
    pub(super) fn script_box_ok_for_test(&mut self, ctx: &Context) {
        let outcome = match &self.dialog {
            Dialog::Script(ScriptBox::SaveMacro { name, source, .. }) => BoxOutcome::SaveMacro(name.clone(), source.clone()),
            Dialog::Script(ScriptBox::NewScript { name, .. }) => BoxOutcome::NewScript(name.clone()),
            Dialog::Script(ScriptBox::Rename { path, name, .. }) => BoxOutcome::Rename(path.clone(), name.clone()),
            Dialog::Script(ScriptBox::Delete { path, .. }) => BoxOutcome::Delete(path.clone()),
            _ => BoxOutcome::Nothing,
        };
        self.box_outcome(outcome, ctx);
    }

    /// Plays what a box has just decided on, now there's a frame to play it in.
    pub(super) fn start_pending_play(&mut self, ui: &Ui) {
        if let Some((path, repeat)) = self.pending_play.take() {
            match path {
                Some(path) => self.play_path(&path, repeat, ui),
                None => self.play_current_macro(repeat, ui),
            }
        }
    }

    /// A script that's taking a while says so, with a way to stop it.
    fn running_window(&mut self, ctx: &Context) {
        let Some(run) = &self.script else {
            return;
        };
        let waited = run.run.started.elapsed();
        if waited < SHOW_RUNNING_AFTER || !matches!(self.dialog, Dialog::None) {
            return;
        }
        let name = run.run.name.clone();
        let mut stop = ctx.input(|i| i.key_pressed(Key::Escape));
        egui::Window::new("Playing").collapsible(false).resizable(false).max_width(280.0).anchor(egui::Align2::RIGHT_BOTTOM, [-20.0, -36.0]).show(ctx, |ui| {
            ui.label(format!("{name}: {} seconds so far", waited.as_secs()));
            ui.add_space(4.0);
            stop |= ui.button("Stop").on_hover_text("Stops it where it is (Esc). One Undo takes back what it had done").clicked();
        });
        if stop {
            self.stop_script();
        }
    }

    // ---------- The Advanced tab ----------

    /// Sets up a state to take a picture of. A screenshot run writes nothing that matters.
    pub(super) fn script_screenshot_step(&mut self, step: &str) {
        match step {
            "macro-panels" => {
                self.panels.macros = true;
                self.panels.scripts = true;
                let _ = self.library.save(Kind::Macro, "Tidy a line", "press(\"Home\");\ntype_text(\"> \");\npress(\"Down\");\n", true);
                self.library.refresh();
                self.current_macro = Some(CurrentMacro { name: "Tidy a line".into(), path: self.library.macros.first().map(|item| item.path.clone()), source: String::new() });
            }
            "macro-record" => scripting::start_recording(),
            "macro-save" => {
                let source = "type_text(\"Hello\");\npress(\"Enter\");\ncommand(\"upper_case\");\n".to_string();
                self.dialog = Dialog::Script(ScriptBox::SaveMacro { source, name: "Macro 1".into(), error: String::new(), focus: true });
            }
            "macro-times" => self.ask_how_many_times(None),
            "addons" | "addon-panel" => {
                // Some add-ons to show, made in a folder of their own: one on, one off, one with a mistake, one shared.
                let root = std::env::temp_dir().join("limoniter-addon-shot");
                let _ = std::fs::remove_dir_all(&root);
                let mut addons = crate::addons::AddOns::new(Some(root.clone()));
                let tidy = addons.create("Tidy CSV").unwrap_or_default();
                let _ = std::fs::write(&tidy, "//! name: Tidy CSV\n//! about: Trims the spaces round every field, and checks each row has as many fields as the first.\n//! command: Tidy CSV fields = tidy\n//! command: Check field counts = check\n//! on_save: tidy\n");
                let _ = addons.create("Word list");
                let _ = std::fs::write(root.join(crate::addons::FOLDER).join("Broken.rhai"), "//! name: Half finished\n//! command: Do it\n");
                let team = root.join("team");
                let _ = std::fs::create_dir_all(&team);
                let _ = std::fs::write(team.join("Header.rhai"), "//! name: Company header\n//! about: Puts the standard copyright header at the top of new source files.\n//! on_open: add_header\n");
                addons.set_shared(Some(team));
                addons.set_on(&tidy, true);
                self.addons = addons;
                if step == "addons" {
                    self.dialog = Dialog::AddOns { shared: self.addons.shared.as_ref().map_or_else(String::new, |f| f.display().to_string()) };
                } else {
                    self.show_addon_panel("TODOs in notes.txt".into(), "3: TODO check the totals\n17: TODO ask about the dates\n42: TODO tidy this up\n", false);
                }
            }
            "script-running" => {
                if let Ok(run) = scripting::start("Endless loop", "loop { let a = 1; }", Repeat::Once) {
                    self.script = Some(Box::new(ScriptRun { run, waiting: None, printed: false, after: None }));
                }
            }
            "script-output" => {
                self.script_print("Line 12: 86 characters".into(), true);
                self.script_print("Line 3: 71 characters".into(), false);
            }
            _ => {}
        }
    }

    /// What a script or a command has printed, in the Output window.
    pub(super) fn script_output_panel(&mut self, ui: &mut Ui) {
        let mut go_to: Option<(String, usize)> = None;
        let mut close = false;
        let mut stop = false;
        let running = self.tool_running();
        Panel::bottom("results-panel").default_size(190.0).resizable(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                let lines = self.script_output.len();
                ui.label(format!("Output: {lines} line{}{}", if lines == 1 { "" } else { "s" }, if running { ", running" } else { "" }));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.small_button("\u{D7}").on_hover_text("Close").clicked();
                    if running {
                        stop = ui.small_button("Stop").on_hover_text("Ends the command, and anything it started").clicked();
                    }
                    if ui.small_button("Copy all").clicked() {
                        ui.ctx().copy_text(self.script_output.join("\n"));
                    }
                });
            });
            ui.separator();
            let row = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
            egui::ScrollArea::both().id_salt("script-output").auto_shrink([false, false]).stick_to_bottom(true).show_rows(ui, row, self.script_output.len(), |ui, range| {
                for line in &self.script_output[range] {
                    // A place, as compilers and assemblers print them, goes there when clicked.
                    match crate::assemblers::problem_line(line) {
                        Some((file, number, _)) => {
                            if ui.add(egui::Label::new(RichText::new(line).monospace().color(theme::accent())).wrap_mode(egui::TextWrapMode::Extend).sense(egui::Sense::click())).on_hover_text("Go to this line").clicked() {
                                go_to = Some((file, number));
                            }
                        }
                        None => {
                            ui.add(egui::Label::new(RichText::new(line).monospace()).wrap_mode(egui::TextWrapMode::Extend));
                        }
                    }
                }
            });
        });
        if stop || (close && running) {
            self.stop_tool();
        }
        if let Some((file, line)) = go_to {
            self.go_to_place(&file, line);
        }
        if close {
            self.script_output.clear();
            self.show_results = false;
            self.pending_focus = true;
        }
    }
}
