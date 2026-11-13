//! The assistant's history: every request made of Claude Code or Codex, what came back, and what was done with it,
//! kept in the data folder between sessions so it can always be looked back on.

use super::assistant::{Task, Tool};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The most requests kept.
const MOST_KEPT: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
    /// The tool is still at work.
    Working,
    /// Words came back, to read.
    Answered,
    /// Code came back, and is waiting to be looked at.
    Suggested,
    PutIn,
    Left,
    Failed,
    Stopped,
}

impl State {
    fn id(self) -> &'static str {
        match self {
            State::Working => "working",
            State::Answered => "answered",
            State::Suggested => "suggested",
            State::PutIn => "put-in",
            State::Left => "left",
            State::Failed => "failed",
            State::Stopped => "stopped",
        }
    }

    fn from_id(id: &str) -> State {
        match id {
            "answered" => State::Answered,
            "suggested" => State::Suggested,
            "put-in" => State::PutIn,
            "left" => State::Left,
            "failed" => State::Failed,
            // A request still working when Limoniter closed never finished.
            _ => State::Stopped,
        }
    }

    /// In words, for the panel.
    pub fn word(self) -> &'static str {
        match self {
            State::Working => "Working",
            State::Answered => "Answered",
            State::Suggested => "Waiting for you to look",
            State::PutIn => "Put in",
            State::Left => "Left out",
            State::Failed => "Didn't work",
            State::Stopped => "Stopped",
        }
    }
}

/// One request, and what came of it.
#[derive(Clone, Debug)]
pub(super) struct Exchange {
    pub id: u64,
    /// When it was asked, as "2026-11-13 14:02".
    pub when: String,
    pub tool: Tool,
    pub task: Task,
    pub words: String,
    pub file: String,
    pub path: Option<PathBuf>,
    /// The lines it was about, from 0, end not included; empty for the place after a line.
    pub lines: std::ops::Range<usize>,
    pub state: State,
    /// What came back: the answer, or what went wrong.
    pub reply: String,
    /// For code: the part as it was sent, and as it came back.
    pub old: Vec<String>,
    pub new: Vec<String>,
    /// How long it took.
    pub seconds: u64,
    /// The document it came from, and its text when it was sent, while that's open: so a suggestion left out can
    /// still be put in where the part is now.
    pub doc: Option<u64>,
    pub text: Option<Vec<Arc<str>>>,
}

impl Exchange {
    /// Where it was, in words: "lines 10 to 24 of main.rs".
    pub fn place(&self) -> String {
        if self.task == Task::CommitMessage {
            format!("the changes in {}", self.file)
        } else if self.lines.is_empty() {
            format!("after line {} of {}", self.lines.start, self.file)
        } else if self.lines.len() == 1 {
            format!("line {} of {}", self.lines.start + 1, self.file)
        } else {
            format!("lines {} to {} of {}", self.lines.start + 1, self.lines.end, self.file)
        }
    }

    /// For a suggestion: how many lines came and went.
    pub fn counts(&self) -> (usize, usize) {
        let rows = super::assistant::diff_rows(&self.old, &self.new);
        (rows.iter().filter(|(mark, _)| *mark == '+').count(), rows.iter().filter(|(mark, _)| *mark == '-').count())
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "when": self.when,
            "tool": match self.tool { Tool::Claude => "claude", Tool::Codex => "codex" },
            "task": self.task.label(),
            "words": self.words,
            "file": self.file,
            "path": self.path.as_ref().map(|p| p.display().to_string()),
            "start": self.lines.start,
            "end": self.lines.end,
            "state": self.state.id(),
            "reply": self.reply,
            "old": self.old,
            "new": self.new,
            "seconds": self.seconds,
        })
    }

    fn from_json(value: &serde_json::Value) -> Option<Exchange> {
        let text = |key: &str| value.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let number = |key: &str| value.get(key).and_then(serde_json::Value::as_u64).unwrap_or(0);
        let lines = |key: &str| value.get(key).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|l| l.as_str().map(str::to_string)).collect()).unwrap_or_default();
        let task = Task::EVERY.into_iter().find(|task| task.label() == text("task"))?;
        Some(Exchange {
            id: number("id"),
            when: text("when"),
            tool: if text("tool") == "codex" { Tool::Codex } else { Tool::Claude },
            task,
            words: text("words"),
            file: text("file"),
            path: value.get("path").and_then(|v| v.as_str()).map(PathBuf::from),
            lines: number("start") as usize..number("end") as usize,
            state: State::from_id(&text("state")),
            reply: text("reply"),
            old: lines("old"),
            new: lines("new"),
            seconds: number("seconds"),
            doc: None,
            text: None,
        })
    }
}

/// The history, and where it's kept.
#[derive(Default)]
pub(super) struct Log {
    pub entries: Vec<Exchange>,
    path: Option<PathBuf>,
    next: u64,
}

impl Log {
    /// The history kept in a folder, or none kept (in a test) with None.
    pub fn open(folder: Option<&Path>) -> Log {
        let path = folder.map(|f| f.join("assistant-history.json"));
        let entries: Vec<Exchange> = path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| value.as_array().map(|items| items.iter().filter_map(Exchange::from_json).collect()))
            .unwrap_or_default();
        let next = entries.iter().map(|e| e.id).max().map_or(1, |id| id + 1);
        Log { entries, path, next }
    }

    /// Starts an entry for a request, and says which it is.
    pub fn add(&mut self, mut exchange: Exchange) -> u64 {
        exchange.id = self.next;
        self.next += 1;
        let id = exchange.id;
        self.entries.push(exchange);
        if self.entries.len() > MOST_KEPT {
            self.entries.drain(..self.entries.len() - MOST_KEPT);
        }
        self.save();
        id
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Exchange> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Changes an entry, and keeps the history.
    pub fn update(&mut self, id: u64, change: impl FnOnce(&mut Exchange)) {
        if let Some(entry) = self.get_mut(id) {
            change(entry);
            self.save();
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.save();
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let all: Vec<serde_json::Value> = self.entries.iter().map(Exchange::to_json).collect();
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let _ = crate::codec::write_file(path, serde_json::to_string_pretty(&all).unwrap_or_default().as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_history_is_kept_between_sessions() {
        let folder = std::env::temp_dir().join(format!("limoniter-assist-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let mut log = Log::open(Some(&folder));
        let exchange = Exchange { id: 0, when: "2026-11-13 14:02".into(), tool: Tool::Codex, task: Task::Fix, words: "it crashes".into(), file: "main.rs".into(), path: Some(folder.join("main.rs")), lines: 9..24, state: State::Working, reply: String::new(), old: vec!["a".into()], new: vec!["b".into()], seconds: 0, doc: Some(3), text: None };
        let id = log.add(exchange);
        log.update(id, |e| {
            e.state = State::PutIn;
            e.seconds = 12;
        });
        let again = Log::open(Some(&folder));
        assert_eq!(again.entries.len(), 1);
        let kept = &again.entries[0];
        assert_eq!((kept.tool, kept.task, kept.state, kept.seconds, kept.lines.clone()), (Tool::Codex, Task::Fix, State::PutIn, 12, 9..24));
        assert_eq!(kept.place(), "lines 10 to 24 of main.rs");
        assert_eq!(kept.counts(), (1, 1));
        assert!(kept.doc.is_none(), "which document it was is only known while it's open");
        // A request still working when it closed comes back as stopped; new ones carry on from the last number.
        let mut log = Log::open(Some(&folder));
        let working = log.add(Exchange { state: State::Working, ..kept.clone() });
        assert!(working > id);
        assert_eq!(Log::open(Some(&folder)).entries.last().unwrap().state, State::Stopped);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
