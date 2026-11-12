//! Macros and scripts, which are one thing here: a recorded macro is a script anyone can read and change.
//!
//! They're written in Rhai, a small language made for putting inside programs. It has no way of its own to reach files,
//! other programs or the network, so what a script can do is exactly what's offered here.
//!
//! A script runs on its own thread and asks the window to do each thing, so however long it takes, or if it never ends,
//! the window stays alive and its Stop button works. Nothing is shared: questions and answers go back and forth.

use crate::codec::TextFormat;
use crate::text_buffer::TextBuffer;
use crate::theme;
use crate::views::{self, ViewState};
use eframe::egui::{self, Key, Modifiers, RichText, Ui};
use rhai::{Dynamic, Engine, EvalAltResult, ImmutableString, Map, Position};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

pub const EXTENSION: &str = "rhai";
/// The most text a script can hold in one piece, which stops a loop that keeps doubling a string.
const MOST_STRING_BYTES: usize = 128 * 1024 * 1024;
const MOST_ARRAY_ITEMS: usize = 5_000_000;
const MOST_MAP_ITEMS: usize = 1_000_000;
/// How deeply functions can call each other. There's stack enough for this on the script's thread.
const MOST_CALL_LEVELS: usize = 64;
const SCRIPT_STACK_BYTES: usize = 64 * 1024 * 1024;
/// However a macro behaves, "until the end of the file" comes to an end.
const MOST_UNTIL_END_RUNS: u64 = 10_000_000;
/// The whole of a file that a script can ask for at once. Longer ones can be read a line at a time.
const MOST_TEXT_BYTES: usize = 64 * 1024 * 1024;

/// How often a macro plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Once,
    Times(u64),
    /// Until a run doesn't move the cursor further down, and doesn't remove any lines.
    UntilEnd,
}

/// Which words a search goes by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Search {
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub backward: bool,
}

/// Something a script wants done or wants to know.
#[derive(Debug, PartialEq)]
pub enum Call {
    LineCount,
    Line(i64),
    CurrentLine,
    Column,
    Text,
    Selected,
    HasSelection,
    GoTo(i64, i64),
    GoToPlace(String),
    Select(i64, i64, i64, i64),
    SelectAll,
    Press(String, i64),
    Type(String),
    SetLine(i64, String),
    InsertLine(i64, String),
    DeleteLine(i64),
    SetText(String),
    Find(String, Search),
    ReplaceNext(String, String, Search),
    ReplaceAll(String, String, Search),
    Command(String),
    Message(String),
    Ask(String, String),
    Confirm(String),
    Print(String),
    NewFile,
    OpenFile(String),
    Save,
    SaveAs(String),
    NextFile,
    PreviousFile,
    FileCount,
    FileName,
    FilePath,
    IsModified,
    Clipboard,
    SetClipboard(String),
    /// Shows text in a panel of its own, by title, replacing what it showed; or adds to it.
    Panel(String, String, bool),
    /// The device a file is about to be sent to, for an add-on's on_send; empty otherwise.
    SendDevice,
    /// The name the file is sent under.
    SetSendName(String),
}

#[derive(Debug, PartialEq)]
pub enum Answer {
    Nothing,
    Yes(bool),
    Number(i64),
    Text(String),
}

type Reply = Result<Answer, String>;

enum FromScript {
    Call(Call),
    Finished(Result<(), String>),
}

/// What's next from a running script.
pub enum Next {
    Call(Call),
    Finished(Result<(), String>),
    /// It's busy working something out.
    Nothing,
}

/// A script underway.
pub struct Run {
    pub name: String,
    pub started: Instant,
    /// The script itself uses undo, so what it does has to be kept step by step until it ends.
    pub keep_steps: bool,
    from_script: Receiver<FromScript>,
    to_script: Sender<Reply>,
    stop: Arc<AtomicBool>,
}

impl Run {
    pub fn next(&self, wait: Duration) -> Next {
        match self.from_script.recv_timeout(wait) {
            Ok(FromScript::Call(call)) => Next::Call(call),
            Ok(FromScript::Finished(result)) => Next::Finished(result),
            Err(RecvTimeoutError::Timeout) => Next::Nothing,
            Err(RecvTimeoutError::Disconnected) => Next::Finished(Err("It stopped unexpectedly.".into())),
        }
    }

    pub fn answer(&self, reply: Reply) {
        let _ = self.to_script.send(reply);
    }
}

impl Drop for Run {
    /// However it ends, the script's thread stops at its next step.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Starts a script on its own thread. It does nothing to the file until the window answers its calls.
pub fn start(name: &str, source: &str, repeat: Repeat) -> Result<Run, String> {
    start_with(name, source, repeat, None)
}

/// Starts one function of a script, as an add-on's command or event does. Whatever is at the top of the script
/// runs first, so it can set up what its functions share.
pub fn start_function(name: &str, source: &str, function: &str) -> Result<Run, String> {
    start_with(name, source, Repeat::Once, Some(function.to_string()))
}

fn start_with(name: &str, source: &str, repeat: Repeat, function: Option<String>) -> Result<Run, String> {
    let (to_host, from_script) = mpsc::channel();
    let (to_script, from_host) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let code = source.to_string();
    std::thread::Builder::new()
        .name("limoniter-script".into())
        .stack_size(SCRIPT_STACK_BYTES)
        .spawn(move || {
            let result = perform(&code, repeat, function.as_deref(), to_host.clone(), from_host, thread_stop);
            let _ = to_host.send(FromScript::Finished(result));
        })
        .map_err(|e| format!("It couldn't be started: {e}"))?;
    let keep_steps = source.contains("\"undo\"") || source.contains("\"redo\"");
    Ok(Run { name: name.to_string(), started: Instant::now(), keep_steps, from_script, to_script, stop })
}

fn perform(code: &str, repeat: Repeat, function: Option<&str>, to_host: Sender<FromScript>, from_host: Receiver<Reply>, stop: Arc<AtomicBool>) -> Result<(), String> {
    let bridge = Rc::new(Bridge { to_host, from_host, stop });
    let engine = build_engine(&bridge);
    let ast = engine.compile(code).map_err(|e| {
        let place = e.position();
        match place.line() {
            Some(line) => format!("There's a mistake on line {line}: {}", e.err_type()),
            None => format!("There's a mistake in it: {}", e.err_type()),
        }
    })?;
    if let Some(function) = function {
        if !ast.iter_functions().any(|f| f.name == function && f.params.is_empty()) {
            return Err(format!("It has no function called {function}() that takes nothing."));
        }
        let options = rhai::CallFnOptions::new().eval_ast(true);
        return engine.call_fn_with_options::<Dynamic>(options, &mut rhai::Scope::new(), &ast, function, ()).map(|_| ()).map_err(|e| describe(&e));
    }
    let once = || engine.run_ast(&ast).map_err(|e| describe(&e));
    match repeat {
        Repeat::Once => once(),
        Repeat::Times(times) => (0..times).try_for_each(|_| once()),
        Repeat::UntilEnd => {
            let place = || -> Result<(i64, i64), String> { Ok((bridge.number(Call::CurrentLine).map_err(|e| describe(&e))?, bridge.number(Call::LineCount).map_err(|e| describe(&e))?)) };
            for _ in 0..MOST_UNTIL_END_RUNS {
                let (line, count) = place()?;
                once()?;
                let (new_line, new_count) = place()?;
                if new_line <= line && new_count >= count {
                    break;
                }
            }
            Ok(())
        }
    }
}

/// Says what went wrong in ordinary words, and where.
fn describe(error: &EvalAltResult) -> String {
    let place = error.position().line().map_or_else(String::new, |line| format!(" (line {line})"));
    match error {
        EvalAltResult::ErrorRuntime(message, _) => format!("{message}{place}"),
        EvalAltResult::ErrorTerminated(..) => "It was stopped.".to_string(),
        EvalAltResult::ErrorDataTooLarge(what, _) => format!("It tried to hold too much: {what}{place}"),
        EvalAltResult::ErrorStackOverflow(_) => format!("Its functions call each other too deeply{place}"),
        EvalAltResult::ErrorInFunctionCall(_, _, inner, _) => describe(inner),
        other => {
            let text = other.to_string();
            match text.split_once(" (line ") {
                Some((before, _)) => format!("{before}{place}"),
                None => format!("{text}{place}"),
            }
        }
    }
}

/// The script's end of the conversation with the window.
struct Bridge {
    to_host: Sender<FromScript>,
    from_host: Receiver<Reply>,
    stop: Arc<AtomicBool>,
}

type Fails<T> = Result<T, Box<EvalAltResult>>;

fn failure<T>(message: String) -> Fails<T> {
    Err(Box::new(EvalAltResult::ErrorRuntime(message.into(), Position::NONE)))
}

impl Bridge {
    fn ask(&self, call: Call) -> Fails<Answer> {
        let stopped = || Box::new(EvalAltResult::ErrorTerminated(Dynamic::UNIT, Position::NONE));
        if self.stop.load(Ordering::Relaxed) {
            return Err(stopped());
        }
        self.to_host.send(FromScript::Call(call)).map_err(|_| stopped())?;
        match self.from_host.recv() {
            Ok(Ok(answer)) => Ok(answer),
            Ok(Err(message)) => failure(message),
            Err(_) => Err(stopped()),
        }
    }

    fn done(&self, call: Call) -> Fails<()> {
        self.ask(call).map(|_| ())
    }

    fn yes(&self, call: Call) -> Fails<bool> {
        match self.ask(call)? {
            Answer::Yes(yes) => Ok(yes),
            other => failure(format!("Limoniter gave an answer it shouldn't have: {other:?}")),
        }
    }

    fn number(&self, call: Call) -> Fails<i64> {
        match self.ask(call)? {
            Answer::Number(number) => Ok(number),
            other => failure(format!("Limoniter gave an answer it shouldn't have: {other:?}")),
        }
    }

    fn text(&self, call: Call) -> Fails<String> {
        match self.ask(call)? {
            Answer::Text(text) => Ok(text),
            other => failure(format!("Limoniter gave an answer it shouldn't have: {other:?}")),
        }
    }
}

/// Reads a map like `#{ regex: true, match_case: true }`. A word it doesn't know is a mistake worth pointing out.
fn search_from(options: &Map) -> Fails<Search> {
    let mut search = Search::default();
    for (name, value) in options {
        let on = value.as_bool().or_else(|_| failure(format!("{name} needs to be true or false.")))?;
        match name.as_str() {
            "match_case" => search.match_case = on,
            "whole_word" => search.whole_word = on,
            "regex" => search.regex = on,
            "backward" => search.backward = on,
            other => return failure(format!("A search has no option called {other}. It can have match_case, whole_word, regex and backward.")),
        }
    }
    Ok(search)
}

fn pattern(text: &str) -> Fails<regex::Regex> {
    regex::Regex::new(text).or_else(|e| failure(format!("That regular expression isn't right: {e}")))
}

/// Binds one name to a call, keeping a line for each of the many there are.
macro_rules! offer {
    ($engine:ident, $bridge:ident, $name:literal, $how:ident, |$($arg:ident : $kind:ty),*| $call:expr) => {{
        let bridge = $bridge.clone();
        $engine.register_fn($name, move |$($arg: $kind),*| bridge.$how($call));
    }};
}

fn build_engine(bridge: &Rc<Bridge>) -> Engine {
    let mut engine = Engine::new();
    // Rhai can load other script files by itself unless told not to, and can run text as code.
    engine.set_module_resolver(rhai::module_resolvers::DummyModuleResolver::new());
    engine.disable_symbol("eval");
    engine.set_max_call_levels(MOST_CALL_LEVELS);
    engine.set_max_expr_depths(64, 64);
    engine.set_max_string_size(MOST_STRING_BYTES);
    engine.set_max_array_size(MOST_ARRAY_ITEMS);
    engine.set_max_map_size(MOST_MAP_ITEMS);
    let stop = bridge.stop.clone();
    engine.on_progress(move |_| stop.load(Ordering::Relaxed).then_some(Dynamic::UNIT));
    let printer = bridge.clone();
    engine.on_print(move |text| {
        let _ = printer.done(Call::Print(text.to_string()));
    });
    let debugger = bridge.clone();
    engine.on_debug(move |text, _, _| {
        let _ = debugger.done(Call::Print(text.to_string()));
    });

    offer!(engine, bridge, "line_count", number, | | Call::LineCount);
    offer!(engine, bridge, "line_text", text, |n: i64| Call::Line(n));
    offer!(engine, bridge, "current_line", number, | | Call::CurrentLine);
    offer!(engine, bridge, "current_column", number, | | Call::Column);
    offer!(engine, bridge, "whole_text", text, | | Call::Text);
    offer!(engine, bridge, "selected_text", text, | | Call::Selected);
    offer!(engine, bridge, "has_selection", yes, | | Call::HasSelection);
    offer!(engine, bridge, "go_to", done, |line: i64, column: i64| Call::GoTo(line, column));
    offer!(engine, bridge, "go_to_line", done, |line: i64| Call::GoTo(line, 1));
    offer!(engine, bridge, "go_to_place", done, |place: ImmutableString| Call::GoToPlace(place.to_string()));
    offer!(engine, bridge, "select", done, |from_line: i64, from_column: i64, to_line: i64, to_column: i64| Call::Select(from_line, from_column, to_line, to_column));
    offer!(engine, bridge, "select_all", done, | | Call::SelectAll);
    offer!(engine, bridge, "press", done, |key: ImmutableString| Call::Press(key.to_string(), 1));
    offer!(engine, bridge, "press", done, |key: ImmutableString, times: i64| Call::Press(key.to_string(), times));
    offer!(engine, bridge, "type_text", done, |text: ImmutableString| Call::Type(text.to_string()));
    offer!(engine, bridge, "set_line", done, |n: i64, text: ImmutableString| Call::SetLine(n, text.to_string()));
    offer!(engine, bridge, "insert_line", done, |n: i64, text: ImmutableString| Call::InsertLine(n, text.to_string()));
    offer!(engine, bridge, "delete_line", done, |n: i64| Call::DeleteLine(n));
    offer!(engine, bridge, "set_whole_text", done, |text: ImmutableString| Call::SetText(text.to_string()));
    offer!(engine, bridge, "find", yes, |text: ImmutableString| Call::Find(text.to_string(), Search::default()));
    offer!(engine, bridge, "replace_next", yes, |find: ImmutableString, with: ImmutableString| Call::ReplaceNext(find.to_string(), with.to_string(), Search::default()));
    offer!(engine, bridge, "replace_all", number, |find: ImmutableString, with: ImmutableString| Call::ReplaceAll(find.to_string(), with.to_string(), Search::default()));
    let with_options = bridge.clone();
    engine.register_fn("find", move |text: ImmutableString, options: Map| with_options.yes(Call::Find(text.to_string(), search_from(&options)?)));
    let with_options = bridge.clone();
    engine.register_fn("replace_next", move |find: ImmutableString, with: ImmutableString, options: Map| with_options.yes(Call::ReplaceNext(find.to_string(), with.to_string(), search_from(&options)?)));
    let with_options = bridge.clone();
    engine.register_fn("replace_all", move |find: ImmutableString, with: ImmutableString, options: Map| with_options.number(Call::ReplaceAll(find.to_string(), with.to_string(), search_from(&options)?)));
    offer!(engine, bridge, "command", done, |name: ImmutableString| Call::Command(name.to_string()));
    offer!(engine, bridge, "message", done, |text: ImmutableString| Call::Message(text.to_string()));
    offer!(engine, bridge, "ask", text, |prompt: ImmutableString| Call::Ask(prompt.to_string(), String::new()));
    offer!(engine, bridge, "ask", text, |prompt: ImmutableString, suggested: ImmutableString| Call::Ask(prompt.to_string(), suggested.to_string()));
    offer!(engine, bridge, "confirm", yes, |question: ImmutableString| Call::Confirm(question.to_string()));
    offer!(engine, bridge, "new_file", done, | | Call::NewFile);
    offer!(engine, bridge, "open_file", yes, |path: ImmutableString| Call::OpenFile(path.to_string()));
    offer!(engine, bridge, "save", yes, | | Call::Save);
    offer!(engine, bridge, "save_as", yes, |path: ImmutableString| Call::SaveAs(path.to_string()));
    offer!(engine, bridge, "next_file", done, | | Call::NextFile);
    offer!(engine, bridge, "previous_file", done, | | Call::PreviousFile);
    offer!(engine, bridge, "file_count", number, | | Call::FileCount);
    offer!(engine, bridge, "file_name", text, | | Call::FileName);
    offer!(engine, bridge, "file_path", text, | | Call::FilePath);
    offer!(engine, bridge, "is_modified", yes, | | Call::IsModified);
    offer!(engine, bridge, "clipboard", text, | | Call::Clipboard);
    offer!(engine, bridge, "set_clipboard", done, |text: ImmutableString| Call::SetClipboard(text.to_string()));
    offer!(engine, bridge, "show_panel", done, |title: ImmutableString, text: ImmutableString| Call::Panel(title.to_string(), text.to_string(), false));
    offer!(engine, bridge, "send_device", text, | | Call::SendDevice);
    offer!(engine, bridge, "set_send_name", done, |name: ImmutableString| Call::SetSendName(name.to_string()));
    offer!(engine, bridge, "add_to_panel", done, |title: ImmutableString, text: ImmutableString| Call::Panel(title.to_string(), text.to_string(), true));

    // Regular expressions on text the script is holding, which Rhai doesn't have by itself.
    engine.register_fn("regex_matches", |text: ImmutableString, with: ImmutableString| -> Fails<bool> { Ok(pattern(&with)?.is_match(&text)) });
    engine.register_fn("regex_replace", |text: ImmutableString, with: ImmutableString, by: ImmutableString| -> Fails<String> { Ok(pattern(&with)?.replace_all(&text, by.as_str()).into_owned()) });
    engine.register_fn("regex_find_all", |text: ImmutableString, with: ImmutableString| -> Fails<rhai::Array> {
        Ok(pattern(&with)?.find_iter(&text).map(|found| Dynamic::from(found.as_str().to_string())).collect())
    });
    engine
}

// ---------- What a script does to the text ----------

fn place_on_line(buffer: &TextBuffer, line: i64) -> Result<usize, String> {
    let count = buffer.line_count();
    if line < 1 || line as u64 > count as u64 {
        return Err(format!("There's no line {line}. This file has {count} line{}, counted from 1.", if count == 1 { "" } else { "s" }));
    }
    Ok(line as usize - 1)
}

fn place_in_line(column: i64) -> Result<usize, String> {
    if column < 1 {
        return Err(format!("There's no column {column}. Columns are counted from 1."));
    }
    Ok(column as usize - 1)
}

fn readable(format: &TextFormat, text: &str) -> String {
    format.readable_text(text)
}

fn prepared(buffer: &TextBuffer, text: &str) -> String {
    buffer.format.prepare_input(text, buffer.input_table())
}

fn rows_of(buffer: &TextBuffer, text: &str) -> Vec<String> {
    prepared(buffer, text).replace("\r\n", "\n").replace('\r', "\n").split('\n').map(str::to_string).collect()
}

/// Whether a call changes the text, and so can't go ahead on a locked file.
pub fn edits_text(call: &Call) -> bool {
    matches!(call, Call::Type(_) | Call::SetLine(..) | Call::InsertLine(..) | Call::DeleteLine(_) | Call::SetText(_) | Call::ReplaceNext(..) | Call::ReplaceAll(..))
}

/// Whether a call has anything to do with the text of this document. Others are for the window to deal with.
pub fn is_about_text(call: &Call) -> bool {
    edits_text(call)
        || matches!(
            call,
            Call::LineCount | Call::Line(_) | Call::CurrentLine | Call::Column | Call::Text | Call::Selected | Call::HasSelection | Call::GoTo(..) | Call::GoToPlace(_) | Call::Select(..) | Call::SelectAll | Call::Press(..) | Call::Find(..)
        )
}

/// Does whatever only takes this document and its view. Searches are done by the window, which knows about big files.
pub fn on_text(buffer: &mut TextBuffer, state: &mut ViewState, call: Call) -> Reply {
    let answer = match call {
        Call::LineCount => Answer::Number(buffer.line_count() as i64),
        Call::Line(line) => {
            let at = place_on_line(buffer, line)?;
            Answer::Text(readable(&buffer.format, &buffer.line(at)))
        }
        Call::CurrentLine => Answer::Number(buffer.caret_char_position().0 as i64 + 1),
        Call::Column => Answer::Number(buffer.caret_char_position().1 as i64 + 1),
        Call::Text => {
            if buffer.is_disk_backed() {
                return Err("This file is too large to read all at once. It is being kept on disk, so read it with line_text.".into());
            }
            let lines = buffer.all_lines();
            if lines.iter().map(|line| line.len() + 1).sum::<usize>() > MOST_TEXT_BYTES {
                return Err("This file is too large to read all at once, so read it with line_text.".into());
            }
            Answer::Text(lines.iter().map(|line| readable(&buffer.format, line)).collect::<Vec<_>>().join("\n"))
        }
        Call::Selected => {
            if buffer.selection_too_large_to_copy() {
                return Err("That selection is too large to read all at once.".into());
            }
            Answer::Text(readable(&buffer.format, &buffer.selection_text()).replace("\r\n", "\n"))
        }
        Call::HasSelection => Answer::Yes(buffer.has_selection()),
        Call::GoTo(line, column) => {
            let (line, column) = (place_on_line(buffer, line)?, place_in_line(column)?);
            buffer.move_caret_to_char(line, column);
            Answer::Nothing
        }
        Call::GoToPlace(place) => {
            if !buffer.go_to(&place) {
                return Err(format!("There's no place called {place} in this file."));
            }
            Answer::Nothing
        }
        Call::Select(from_line, from_column, to_line, to_column) => {
            let from = (place_on_line(buffer, from_line)?, place_in_line(from_column)?);
            let to = (place_on_line(buffer, to_line)?, place_in_line(to_column)?);
            buffer.select_chars(from, to);
            Answer::Nothing
        }
        Call::SelectAll => {
            buffer.select_all();
            Answer::Nothing
        }
        Call::Press(name, times) => {
            let (key, modifiers) = key_named(&name)?;
            for _ in 0..times.max(0) {
                views::press_key(buffer, state, key, modifiers);
            }
            Answer::Nothing
        }
        Call::Type(text) => {
            buffer.type_text(&text);
            Answer::Nothing
        }
        Call::SetLine(line, text) => {
            let at = place_on_line(buffer, line)?;
            let rows = rows_of(buffer, &text);
            buffer.set_line_keeping_cursor(at, rows);
            Answer::Nothing
        }
        Call::InsertLine(line, text) => {
            let count = buffer.line_count() as i64;
            if line < 1 || line > count + 1 {
                return Err(format!("A line can go in from 1 to {}, which is after the last.", count + 1));
            }
            let rows = rows_of(buffer, &text);
            buffer.insert_lines_keeping_cursor(line as usize - 1, rows);
            Answer::Nothing
        }
        Call::DeleteLine(line) => {
            let at = place_on_line(buffer, line)?;
            buffer.delete_line_keeping_cursor(at);
            Answer::Nothing
        }
        Call::SetText(text) => {
            if buffer.is_disk_backed() {
                return Err("This file is too large to replace all at once. It is being kept on disk.".into());
            }
            buffer.select_all();
            if text.is_empty() {
                buffer.delete_selection();
            } else {
                buffer.type_text(&text);
            }
            Answer::Nothing
        }
        other => return Err(format!("Limoniter couldn't do this here: {other:?}")),
    };
    Ok(answer)
}

/// Which key a name means, like "Enter" or "Ctrl+Shift+Left".
pub fn key_named(name: &str) -> Result<(Key, Modifiers), String> {
    let mut modifiers = Modifiers::NONE;
    let mut parts: Vec<&str> = name.split('+').map(str::trim).collect();
    let key = parts.pop().unwrap_or_default();
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" => modifiers |= Modifiers::CTRL,
            "shift" => modifiers |= Modifiers::SHIFT,
            "alt" => modifiers |= Modifiers::ALT,
            other => return Err(format!("{other} isn't a key that can be held down. Use Ctrl, Shift or Alt.")),
        }
    }
    KEYS.iter().find(|(known, _)| known.eq_ignore_ascii_case(key)).map(|(_, key)| (*key, modifiers)).ok_or_else(|| format!("press doesn't know a key called {key}. It knows {}.", KEYS.map(|(known, _)| known).join(", ")))
}

const KEYS: [(&str, Key); 14] = [
    ("Left", Key::ArrowLeft),
    ("Right", Key::ArrowRight),
    ("Up", Key::ArrowUp),
    ("Down", Key::ArrowDown),
    ("PageUp", Key::PageUp),
    ("PageDown", Key::PageDown),
    ("Home", Key::Home),
    ("End", Key::End),
    ("Backspace", Key::Backspace),
    ("Delete", Key::Delete),
    ("Enter", Key::Enter),
    ("Tab", Key::Tab),
    ("Insert", Key::Insert),
    ("Escape", Key::Escape),
];

// ---------- Recording ----------

enum Step {
    Type(String),
    Press(String, u64),
    Line(String),
}

thread_local! {
    /// What's been done since recording began. The window and its text view both add to it, on the one thread.
    static RECORDER: RefCell<Option<Vec<Step>>> = const { RefCell::new(None) };
}

pub fn recording() -> bool {
    RECORDER.with_borrow(Option::is_some)
}

pub fn start_recording() {
    RECORDER.set(Some(Vec::new()));
}

/// Ends recording, with the script it made, or None if nothing was done.
pub fn stop_recording() -> Option<String> {
    let steps = RECORDER.take()?;
    if steps.is_empty() {
        return None;
    }
    let mut source = String::new();
    for step in steps {
        match step {
            Step::Type(text) => source += &format!("type_text({});\n", quoted(&text)),
            Step::Press(key, 1) => source += &format!("press(\"{key}\");\n"),
            Step::Press(key, times) => source += &format!("press(\"{key}\", {times});\n"),
            Step::Line(line) => {
                source += &line;
                source.push('\n');
            }
        }
    }
    Some(source)
}

/// Some typed text. Typing runs together into one step, as it reads best that way.
pub fn note_typed(text: &str) {
    RECORDER.with_borrow_mut(|recorder| {
        if let Some(steps) = recorder {
            if let Some(Step::Type(so_far)) = steps.last_mut() {
                so_far.push_str(text);
            } else {
                steps.push(Step::Type(text.to_string()));
            }
        }
    });
}

/// A key that does something in the text.
pub fn note_key(key: Key, modifiers: Modifiers) {
    if !recording() {
        return;
    }
    let Some(name) = KEYS.iter().find(|(_, known)| *known == key).map(|(name, _)| *name) else {
        return;
    };
    // These are Cut, Copy and Paste in disguise, which arrive by themselves.
    if (key == Key::Delete && modifiers.shift) || (key == Key::Insert && (modifiers.shift || modifiers.ctrl)) {
        return;
    }
    let mut full = String::new();
    for (held, word) in [(modifiers.ctrl, "Ctrl+"), (modifiers.shift, "Shift+"), (modifiers.alt, "Alt+")] {
        if held {
            full += word;
        }
    }
    full += name;
    RECORDER.with_borrow_mut(|recorder| {
        if let Some(steps) = recorder {
            if let Some(Step::Press(last, times)) = steps.last_mut()
                && *last == full
            {
                *times += 1;
            } else {
                steps.push(Step::Press(full, 1));
            }
        }
    });
}

/// A line of script to go in as it is.
pub fn note_line(line: String) {
    RECORDER.with_borrow_mut(|recorder| {
        if let Some(steps) = recorder {
            steps.push(Step::Line(line));
        }
    });
}

/// Text as the script language writes it, such that reading it back gives exactly the same.
pub fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out += "\\\"",
            '\\' => out += "\\\\",
            '\n' => out += "\\n",
            '\r' => out += "\\r",
            '\t' => out += "\\t",
            c if c.is_control() || (c as u32) >= 0x10_0000 => out += &format!("\\U{:08X}", c as u32),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A search's options as a script writes them, or nothing if they're all off.
pub fn options_text(search: Search) -> String {
    let words: Vec<&str> = [(search.match_case, "match_case: true"), (search.whole_word, "whole_word: true"), (search.regex, "regex: true"), (search.backward, "backward: true")].iter().filter(|(on, _)| *on).map(|(_, word)| *word).collect();
    if words.is_empty() { String::new() } else { format!(", #{{ {} }}", words.join(", ")) }
}

// ---------- The Macros and Scripts panels ----------

/// What someone did in a panel.
pub enum LibraryAction {
    Play(PathBuf),
    PlayTimes(PathBuf),
    Edit(PathBuf),
    Rename(PathBuf),
    Delete(PathBuf),
    SetKey(PathBuf, Option<u8>),
    Record,
    New,
    LoadMacro,
    LoadScript,
}

#[derive(Default)]
pub struct PanelState {
    selected: Option<PathBuf>,
}

pub fn panel(ui: &mut Ui, kind: Kind, state: &mut PanelState, library: &Library, actions: &mut Vec<LibraryAction>) {
    ui.horizontal(|ui| match kind {
        Kind::Macro => {
            if ui.button(if recording() { "Stop recording" } else { "Record" }).on_hover_text("Record what you type and which commands you use").clicked() {
                actions.push(LibraryAction::Record);
            }
            if ui.button("Load...").on_hover_text("Add a macro from a file").clicked() {
                actions.push(LibraryAction::LoadMacro);
            }
        }
        Kind::Script => {
            if ui.button("New...").on_hover_text("Start a new script, with notes on what scripts can do").clicked() {
                actions.push(LibraryAction::New);
            }
            if ui.button("Load...").on_hover_text("Add a script from a file").clicked() {
                actions.push(LibraryAction::LoadScript);
            }
        }
    });
    ui.add_space(2.0);
    let items = library.items(kind);
    if items.is_empty() {
        let hint = match kind {
            Kind::Macro => "No macros yet. Press Record, do what you want repeated, then press it again.",
            Kind::Script => "No scripts yet. Press New to write one.",
        };
        ui.label(RichText::new(hint).color(theme::text_faint()).size(12.5));
        return;
    }
    if state.selected.as_ref().is_some_and(|selected| !items.iter().any(|item| item.path == *selected)) {
        state.selected = None;
    }
    // The buttons for what's chosen stay in reach under the list, however long it is.
    let chosen = state.selected.clone();
    let buttons_id = match kind {
        Kind::Macro => "macros-buttons",
        Kind::Script => "scripts-buttons",
    };
    egui::Panel::bottom(buttons_id).frame(egui::Frame::new()).show(ui, |ui| {
        ui.add_space(4.0);
        let Some(path) = chosen else {
            ui.label(RichText::new(format!("Choose a {} to change it.", kind.word())).color(theme::text_faint()).size(12.5));
            return;
        };
        ui.horizontal_wrapped(|ui| {
            if ui.button("Edit").on_hover_text("Open it in a tab to change it").clicked() {
                actions.push(LibraryAction::Edit(path.clone()));
            }
            if ui.button("Rename...").clicked() {
                actions.push(LibraryAction::Rename(path.clone()));
            }
            if ui.button("Delete...").clicked() {
                actions.push(LibraryAction::Delete(path.clone()));
            }
            if kind == Kind::Macro && ui.button("Several times...").on_hover_text("Play it a number of times, or to the end of the file").clicked() {
                actions.push(LibraryAction::PlayTimes(path.clone()));
            }
            let held = library.key_of(&path);
            ui.menu_button(held.map_or_else(|| "Key".to_string(), |digit| format!("Ctrl+Alt+{digit}")), |ui| {
                if ui.button("No key").clicked() {
                    actions.push(LibraryAction::SetKey(path.clone(), None));
                    ui.close();
                }
                for digit in 1..=9 {
                    if ui.button(format!("Ctrl+Alt+{digit}")).clicked() {
                        actions.push(LibraryAction::SetKey(path.clone(), Some(digit)));
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("A key to play it with");
        });
    });
    egui::ScrollArea::vertical().id_salt(kind.folder()).auto_shrink([false, false]).show(ui, |ui| {
        for item in items {
            ui.horizontal(|ui| {
                if ui.small_button("Play").on_hover_text("Play it on the file in front").clicked() {
                    actions.push(LibraryAction::Play(item.path.clone()));
                }
                let selected = state.selected.as_deref() == Some(item.path.as_path());
                let label = match library.key_of(&item.path) {
                    Some(digit) => format!("{} (Ctrl+Alt+{digit})", item.name),
                    None => item.name.clone(),
                };
                if ui.selectable_label(selected, label).clicked() {
                    state.selected = Some(item.path.clone());
                }
            });
        }
    });
}

// ---------- Where they're kept ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Macro,
    Script,
}

impl Kind {
    pub fn folder(self) -> &'static str {
        match self {
            Kind::Macro => "macros",
            Kind::Script => "scripts",
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Kind::Macro => "macro",
            Kind::Script => "script",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub path: PathBuf,
}

const LOOK_AGAIN: Duration = Duration::from_secs(2);

/// The macros and scripts on disk, as files anyone can open. A key can be given to any of them.
#[derive(Default)]
pub struct Library {
    /// Where the macros and scripts folders go. None means nothing is kept, as in a test.
    root: Option<PathBuf>,
    pub macros: Vec<Item>,
    pub scripts: Vec<Item>,
    /// Ctrl+Alt and a digit, for which file.
    pub keys: Vec<(u8, PathBuf)>,
    looked: Option<Instant>,
}

impl Library {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self { root, ..Self::default() }
    }

    pub fn items(&self, kind: Kind) -> &[Item] {
        match kind {
            Kind::Macro => &self.macros,
            Kind::Script => &self.scripts,
        }
    }

    pub fn folder(&self, kind: Kind) -> Option<PathBuf> {
        self.root.as_ref().map(|root| root.join(kind.folder()))
    }

    /// Reads the folders again, if it's been a while. Files can come and go from outside.
    pub fn refresh_if_stale(&mut self) {
        if self.looked.is_none_or(|looked| looked.elapsed() >= LOOK_AGAIN) {
            self.refresh();
        }
    }

    pub fn refresh(&mut self) {
        self.looked = Some(Instant::now());
        self.give_starter_scripts();
        self.macros = self.list(Kind::Macro);
        self.scripts = self.list(Kind::Script);
    }

    /// The first time, there are a few scripts to run and to learn from.
    fn give_starter_scripts(&self) {
        let Some(folder) = self.folder(Kind::Script) else {
            return;
        };
        if folder.exists() || std::fs::create_dir_all(&folder).is_err() {
            return;
        }
        for (name, text) in STARTER_SCRIPTS {
            let _ = std::fs::write(folder.join(format!("{name}.{EXTENSION}")), text);
        }
    }

    fn list(&self, kind: Kind) -> Vec<Item> {
        let Some(entries) = self.folder(kind).and_then(|folder| std::fs::read_dir(folder).ok()) else {
            return Vec::new();
        };
        let mut items: Vec<Item> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION)))
            .filter_map(|path| Some(Item { name: path.file_stem()?.to_string_lossy().into_owned(), path }))
            .collect();
        items.sort_by_key(|item| item.name.to_lowercase());
        items
    }

    /// Writes a new one under a name. Nothing is overwritten unless that's asked for.
    pub fn save(&mut self, kind: Kind, name: &str, source: &str, overwrite: bool) -> Result<PathBuf, String> {
        let name = checked_name(name)?;
        let folder = self.folder(kind).ok_or("There's nowhere to keep it, because the settings folder couldn't be found.")?;
        std::fs::create_dir_all(&folder).map_err(|e| format!("The folder couldn't be made: {e}"))?;
        let path = folder.join(format!("{name}.{EXTENSION}"));
        if !overwrite && path.exists() {
            return Err(format!("There's already a {} called {name}.", kind.word()));
        }
        std::fs::write(&path, source).map_err(|e| format!("It couldn't be saved: {e}"))?;
        self.refresh();
        Ok(path)
    }

    /// Copies a script from elsewhere in with the rest, keeping the original where it is.
    pub fn add_copy(&mut self, kind: Kind, from: &Path) -> Result<PathBuf, String> {
        let name = from.file_stem().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if let Some(folder) = self.folder(kind)
            && from.parent().is_some_and(|parent| parent == folder)
        {
            return Ok(from.to_path_buf());
        }
        let source = std::fs::read_to_string(from).map_err(|e| format!("It couldn't be read: {e}"))?;
        self.save(kind, &name, &source, false)
    }

    pub fn rename(&mut self, path: &Path, new_name: &str) -> Result<PathBuf, String> {
        let name = checked_name(new_name)?;
        let target = path.with_file_name(format!("{name}.{EXTENSION}"));
        // A change of capitals alone is the same file as far as Windows is concerned.
        let same = path.to_string_lossy().eq_ignore_ascii_case(&target.to_string_lossy());
        if !same && target.exists() {
            return Err(format!("There's already one called {name}."));
        }
        std::fs::rename(path, &target).map_err(|e| format!("It couldn't be renamed: {e}"))?;
        for (_, key_path) in &mut self.keys {
            if key_path == path {
                key_path.clone_from(&target);
            }
        }
        self.refresh();
        Ok(target)
    }

    pub fn delete(&mut self, path: &Path) -> Result<(), String> {
        std::fs::remove_file(path).map_err(|e| format!("It couldn't be deleted: {e}"))?;
        self.keys.retain(|(_, key_path)| key_path != path);
        self.refresh();
        Ok(())
    }

    pub fn key_of(&self, path: &Path) -> Option<u8> {
        self.keys.iter().find(|(_, key_path)| key_path == path).map(|(digit, _)| *digit)
    }

    /// Gives a key to a file, taking it from anything else that had it.
    pub fn set_key(&mut self, path: &Path, digit: Option<u8>) {
        self.keys.retain(|(known, key_path)| key_path != path && Some(*known) != digit);
        if let Some(digit) = digit {
            self.keys.push((digit, path.to_path_buf()));
            self.keys.sort_by_key(|(digit, _)| *digit);
        }
    }

    pub fn to_settings(&self) -> String {
        self.keys.iter().map(|(digit, path)| format!("script_key={digit}\u{1}{}\n", path.display())).collect()
    }

    pub fn read_settings(&mut self, values: &[String]) {
        for value in values {
            if let Some((digit, path)) = value.split_once('\u{1}')
                && let Ok(digit) = digit.parse::<u8>()
                && (1..=9).contains(&digit)
            {
                self.keys.push((digit, PathBuf::from(path)));
            }
        }
    }
}

/// A name that can only be a plain file name, with no way of climbing out of the folder.
pub fn checked_name(name: &str) -> Result<String, String> {
    let name = name.trim().trim_end_matches('.');
    if name.is_empty() {
        return Err("Give it a name.".into());
    }
    if name.chars().any(|c| matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control()) {
        return Err("A name can't have any of these in it: < > : \" / \\ | ? *".into());
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.ends_with(|c: char| c.is_ascii_digit()));
    if reserved {
        return Err("Windows keeps that name for itself, so choose another.".into());
    }
    Ok(name.to_string())
}

/// What a new script starts with, explaining itself.
pub const NEW_SCRIPT: &str = r#"// A script works on the file in front when it's played. Lines starting // are notes, and are ignored.
// Lines and columns are counted from 1. Undo takes back everything a script did, in one step.
//
// Reading:   line_count()  line_text(n)  current_line()  current_column()  selected_text()  whole_text()
//            has_selection()  file_name()  file_path()  file_count()  is_modified()  clipboard()
// Moving:    go_to(line, column)  go_to_line(n)  select(line, column, line, column)  select_all()  press("Ctrl+Right", 3)
// Changing:  type_text("words")  set_line(n, "words")  insert_line(n, "words")  delete_line(n)  set_whole_text("words")
// Searching: find("words")  replace_next("this", "that")  replace_all("this", "that")
//            and any of them can end , #{ match_case: true, whole_word: true, regex: true, backward: true }
// Text:      regex_matches(text, pattern)  regex_replace(text, pattern, with)  regex_find_all(text, pattern)
// Files:     new_file()  open_file("path")  save()  save_as("path")  next_file()  previous_file()
// Talking:   message("words")  ask("question", "suggestion")  confirm("question")  print("to the Output window")
//            show_panel("title", "words") puts them in a panel of their own; add_to_panel("title", "more") adds to it
// Sending: send_device() names the device, and set_send_name("NAME") changes the file's name, in an add-on's on_send
// Commands:  command("upper_case") does what a button does. A recorded macro shows which ones you used.

let name = ask("What's your name?", "there");
if name != "" {
    message("Hello " + name + ". This file has " + line_count() + " lines.");
}
"#;

const STARTER_SCRIPTS: [(&str, &str); 3] = [
    (
        "Remove blank lines",
        r#"// Removes every empty line, or line of only spaces and tabs, in this file.

let removed = 0;
let line = line_count();
while line >= 1 {
    let text = line_text(line);
    text.trim();
    if text == "" && line_count() > 1 {
        delete_line(line);
        removed += 1;
    }
    line -= 1;
}
print("Removed " + removed + " blank lines from " + file_name());
"#,
    ),
    (
        "Number the lines",
        r#"// Puts a number in front of every line, starting from one you choose.

let first = ask("Start numbering from:", "1");
if first == "" {
    return;
}
let number = parse_int(first);
for line in 1..=line_count() {
    set_line(line, number + ". " + line_text(line));
    number += 1;
}
"#,
    ),
    (
        "List the longest lines",
        r#"// Lists the ten longest lines in the Output window, without changing anything.

let lengths = [];
for line in 1..=line_count() {
    lengths.push(#{ line: line, length: line_text(line).len() });
}
lengths.sort(|a, b| b.length - a.length);
for found in lengths.extract(0, 10) {
    print("Line " + found.line + ": " + found.length + " characters");
}
"#,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a script against a plain buffer, answering everything itself, and hands back how it ended.
    fn run_on(buffer: &mut TextBuffer, source: &str) -> Result<(), String> {
        let run = start("test", source, Repeat::Once).unwrap();
        let mut state = ViewState::default();
        let began = Instant::now();
        loop {
            assert!(began.elapsed() < Duration::from_secs(60), "the script never finished");
            match run.next(Duration::from_millis(50)) {
                Next::Call(call) if is_about_text(&call) => run.answer(on_text(buffer, &mut state, call)),
                Next::Call(Call::Print(_)) => run.answer(Ok(Answer::Nothing)),
                Next::Call(Call::FileName) => run.answer(Ok(Answer::Text("test.txt".into()))),
                Next::Call(other) => panic!("unexpected {other:?}"),
                Next::Finished(result) => return result,
                Next::Nothing => {}
            }
        }
    }

    fn buffer_with(text: &str) -> TextBuffer {
        let mut buffer = TextBuffer::default();
        buffer.set_content(text.split('\n').map(str::to_string).collect());
        buffer
    }

    fn lines(buffer: &TextBuffer) -> Vec<String> {
        buffer.all_lines().iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn a_script_reads_and_changes_lines_counted_from_one() {
        let mut buffer = buffer_with("one\ntwo\nthree");
        run_on(&mut buffer, r#"set_line(2, line_text(1) + " and " + line_text(3)); insert_line(1, "top"); insert_line(line_count() + 1, "bottom"); delete_line(2);"#).unwrap();
        assert_eq!(lines(&buffer), ["top", "one and three", "three", "bottom"]);
    }

    #[test]
    fn asking_for_a_line_that_isnt_there_says_so_and_where() {
        let mut buffer = buffer_with("one");
        let error = run_on(&mut buffer, "let a = 1;\nline_text(9);").unwrap_err();
        assert!(error.contains("no line 9") && error.contains("line 2"), "{error}");
        let error = run_on(&mut buffer, "go_to(1, 0);").unwrap_err();
        assert!(error.contains("no column 0"), "{error}");
        let error = run_on(&mut buffer, r#"press("Banana");"#).unwrap_err();
        assert!(error.contains("Banana") && error.contains("Enter"), "{error}");
    }

    #[test]
    fn a_mistake_in_the_writing_is_found_before_anything_happens() {
        let mut buffer = buffer_with("one");
        let error = run_on(&mut buffer, "set_line(1, \"gone\");\nlet = ;").unwrap_err();
        assert!(error.contains("line 2"), "{error}");
        assert_eq!(lines(&buffer), ["one"]);
    }

    #[test]
    fn keys_play_as_they_do_when_pressed() {
        let mut buffer = buffer_with("alpha\nbeta");
        run_on(&mut buffer, r#"press("End"); type_text("!"); press("Down"); press("Home"); press("Shift+Right", 2); type_text("Z"); press("Ctrl+Home"); press("Delete");"#).unwrap();
        assert_eq!(lines(&buffer), ["lpha!", "Zta"]);
    }

    #[test]
    fn scripts_have_no_way_to_files_programs_or_other_code() {
        let mut buffer = buffer_with("one");
        for attempt in [
            r#"eval("1 + 1");"#,
            r#"import "secret" as s;"#,
            r#"open("C:/secret.txt");"#,
            r#"read_file("C:/secret.txt");"#,
            r#"system("calc");"#,
            r#"exec("calc");"#,
            r#"std::fs::remove_file("x");"#,
        ] {
            assert!(run_on(&mut buffer, attempt).is_err(), "{attempt} should not work");
        }
    }

    #[test]
    fn scripts_cant_eat_all_the_memory_or_overflow_the_stack() {
        let mut buffer = buffer_with("one");
        let error = run_on(&mut buffer, r#"let text = "aaaaaaaaaaaaaaaa"; loop { text += text; }"#).unwrap_err();
        assert!(error.contains("too much"), "{error}");
        let error = run_on(&mut buffer, "let list = [1, 2, 3, 4]; loop { list += list; }").unwrap_err();
        assert!(error.contains("too much"), "{error}");
        let error = run_on(&mut buffer, "let list = [[]]; loop { let more = [list]; more.push(list); list = more; }").unwrap_err();
        assert!(error.contains("too much"), "{error}");
        let error = run_on(&mut buffer, "fn forever(n) { forever(n + 1) } forever(1);").unwrap_err();
        assert!(error.contains("too deeply"), "{error}");
        let deep = format!("let x = {}1{};", "(".repeat(5000), ")".repeat(5000));
        assert!(run_on(&mut buffer, &deep).is_err(), "too deep to read, but no crash");
    }

    #[test]
    fn stopping_ends_a_script_that_would_never_end() {
        let run = start("forever", "loop { let a = 1; }", Repeat::Once).unwrap();
        let stop = run.stop.clone();
        assert!(matches!(run.next(Duration::from_millis(100)), Next::Nothing));
        stop.store(true, Ordering::Relaxed);
        let began = Instant::now();
        loop {
            match run.next(Duration::from_millis(100)) {
                Next::Finished(result) => {
                    assert_eq!(result.unwrap_err(), "It was stopped.");
                    break;
                }
                _ => assert!(began.elapsed() < Duration::from_secs(10), "it should have stopped"),
            }
        }
    }

    #[test]
    fn dropping_a_run_leaves_its_thread_nothing_to_wait_for() {
        // It's waiting on an answer that won't come, and on a loop with nobody listening. Neither may hang or panic.
        let waiting = start("waiting", "line_count();", Repeat::Once).unwrap();
        let _ = waiting.next(Duration::from_secs(5));
        drop(waiting);
        drop(start("spinning", "loop { let a = 1; }", Repeat::Once).unwrap());
    }

    #[test]
    fn text_written_out_reads_back_exactly() {
        let mut buffer = buffer_with("");
        let tricky = "quote \" back \\ tab \t bell \u{7} euro \u{20AC} emoji \u{1F600} kept byte \u{10FF41} {} ${x} `";
        let source = format!("type_text({});", quoted(tricky));
        run_on(&mut buffer, &source).unwrap();
        assert_eq!(lines(&buffer), [tricky]);
        let mut buffer = buffer_with("");
        run_on(&mut buffer, &format!("type_text({});", quoted("two\nlines"))).unwrap();
        assert_eq!(lines(&buffer), ["two", "lines"]);
    }

    #[test]
    fn recording_runs_typing_and_repeated_keys_together() {
        start_recording();
        note_typed("hel");
        note_typed("lo");
        note_key(Key::ArrowDown, Modifiers::NONE);
        note_key(Key::ArrowDown, Modifiers::NONE);
        note_key(Key::ArrowLeft, Modifiers::CTRL | Modifiers::SHIFT);
        note_key(Key::A, Modifiers::CTRL);
        note_key(Key::Delete, Modifiers::SHIFT);
        note_line("command(\"upper_case\");".into());
        note_typed("x\"y");
        assert_eq!(stop_recording().unwrap(), "type_text(\"hello\");\npress(\"Down\", 2);\npress(\"Ctrl+Shift+Left\");\ncommand(\"upper_case\");\ntype_text(\"x\\\"y\");\n");
        assert!(!recording());
        start_recording();
        assert_eq!(stop_recording(), None, "nothing was done");
    }

    #[test]
    fn recording_ignores_whats_done_when_its_off() {
        note_typed("nothing");
        note_key(Key::Enter, Modifiers::NONE);
        note_line("nothing".into());
        assert_eq!(stop_recording(), None);
    }

    #[test]
    fn options_read_back_as_they_were_written() {
        assert_eq!(options_text(Search::default()), "");
        assert_eq!(options_text(Search { regex: true, match_case: true, ..Search::default() }), ", #{ match_case: true, regex: true }");
        let mut buffer = buffer_with("one");
        let error = run_on(&mut buffer, r#"find("a", #{ sideways: true });"#).unwrap_err();
        assert!(error.contains("sideways"), "{error}");
        let error = run_on(&mut buffer, r#"regex_matches("a", "(");"#).unwrap_err();
        assert!(error.contains("regular expression"), "{error}");
    }

    #[test]
    fn regular_expressions_work_on_text_the_script_holds() {
        let mut buffer = buffer_with("");
        run_on(&mut buffer, r##"
            let text = "cat 12 dog 345";
            if regex_matches(text, "\\d+") { type_text(regex_replace(text, "\\d+", "#")); }
            type_text(" " + regex_find_all(text, "\\d+").len());
        "##).unwrap();
        assert_eq!(lines(&buffer), ["cat # dog # 2"]);
    }

    #[test]
    fn a_name_cant_leave_the_folder_or_upset_windows() {
        assert_eq!(checked_name("  Tidy up  ").unwrap(), "Tidy up");
        for bad in ["", "   ", "..", "../up", "a/b", "a\\b", "C:thing", "what?", "CON", "nul.txt", "COM1", "a\u{1}b"] {
            assert!(checked_name(bad).is_err(), "{bad:?}");
        }
        assert!(checked_name("Common").is_ok());
    }

    fn library(tag: &str) -> Library {
        let root = std::env::temp_dir().join(format!("limoniter-script-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Library::new(Some(root))
    }

    #[test]
    fn a_library_keeps_files_lists_them_and_gives_keys() {
        let mut library = library("keep");
        library.refresh();
        assert!(library.macros.is_empty());
        assert_eq!(library.scripts.len(), STARTER_SCRIPTS.len(), "the starter scripts arrive");
        let first = library.save(Kind::Macro, "Beta", "press(\"Enter\");", false).unwrap();
        library.save(Kind::Macro, "alpha", "1;", false).unwrap();
        assert_eq!(library.macros.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(), ["alpha", "Beta"]);
        assert!(library.save(Kind::Macro, "Beta", "other", false).is_err(), "it isn't overwritten without asking");
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "press(\"Enter\");");

        library.set_key(&first, Some(3));
        assert_eq!(library.key_of(&first), Some(3));
        let renamed = library.rename(&first, "Gamma").unwrap();
        assert_eq!(library.key_of(&renamed), Some(3), "the key follows it");
        assert_eq!(library.key_of(&first), None);
        let other = library.macros[0].path.clone();
        library.set_key(&other, Some(3));
        assert_eq!(library.key_of(&renamed), None, "a key goes to one at a time");

        let mut fresh = Library::new(None);
        fresh.read_settings(&library.to_settings().lines().map(|line| line.split_once('=').unwrap().1.to_string()).collect::<Vec<_>>());
        assert_eq!(fresh.keys, library.keys);

        library.delete(&other).unwrap();
        assert_eq!(library.key_of(&other), None);
        assert_eq!(library.macros.len(), 1);
        assert!(library.rename(&renamed, "../out").is_err());
    }

    #[test]
    fn deleted_starter_scripts_stay_deleted() {
        let mut library = library("starters");
        library.refresh();
        let path = library.scripts[0].path.clone();
        library.delete(&path).unwrap();
        library.refresh();
        assert_eq!(library.scripts.len(), STARTER_SCRIPTS.len() - 1);
    }

    #[test]
    fn a_library_with_no_home_holds_nothing_and_says_so() {
        let mut library = Library::new(None);
        library.refresh();
        assert!(library.scripts.is_empty());
        assert!(library.save(Kind::Script, "A", "1;", false).is_err());
    }

    #[test]
    fn the_new_script_and_starter_scripts_all_compile() {
        let engine = Engine::new();
        assert!(engine.compile(NEW_SCRIPT).is_ok());
        for (name, text) in STARTER_SCRIPTS {
            assert!(engine.compile(text).is_ok(), "{name}");
        }
    }

    #[test]
    fn remove_blank_lines_does_what_it_says() {
        let mut buffer = buffer_with("a\n\n   \nb\n\t\n");
        run_on(&mut buffer, STARTER_SCRIPTS[0].1).unwrap();
        assert_eq!(lines(&buffer), ["a", "b"]);
    }

    #[test]
    fn listing_the_longest_lines_changes_nothing() {
        let mut buffer = buffer_with("a\nbbb\ncc");
        run_on(&mut buffer, STARTER_SCRIPTS[2].1).unwrap();
        assert_eq!(lines(&buffer), ["a", "bbb", "cc"]);
    }
}
