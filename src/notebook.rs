//! Jupyter notebooks (`.ipynb`, nbformat 4): reading them into cells, changing the cells, and saving them back the
//! way Jupyter does, so that a notebook that goes through here and comes out again is the same file, byte for byte
//! (and a Git diff of it shows only what was changed).
//!
//! What this keeps: every field it doesn't know, in the notebook, in each cell's metadata and in each output. The
//! outputs stay exactly as the kernel (or the file) gave them, as nbformat 4 output dictionaries.
//! nbformat 3 (IPython 2) notebooks are not supported: they are laid out differently (worksheets), and opening
//! one says so rather than guessing.

use serde_json::{Map, Value};
use std::sync::atomic::{AtomicU64, Ordering};

/// Cells get a number of their own while the notebook is open, so that a running cell is still known after the
/// cells above it have moved. It is never saved.
static NEXT_UID: AtomicU64 = AtomicU64::new(1);

/// The newest nbformat minor version this writes into a new notebook.
const NEW_MINOR: u64 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CellKind {
    Code,
    Markdown,
    Raw,
}

impl CellKind {
    pub fn json_name(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Markdown => "markdown",
            Self::Raw => "raw",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    /// For as long as the notebook is open, and not saved.
    pub uid: u64,
    pub kind: CellKind,
    /// The cell's `id` (nbformat 4.5 and later).
    pub id: Option<String>,
    pub source: String,
    pub metadata: Map<String, Value>,
    /// Code cells only: how many cells had run when it did. `None` before it has run.
    pub execution_count: Option<u64>,
    /// Code cells only: nbformat output dictionaries.
    pub outputs: Vec<Value>,
    /// Markdown and raw cells: pictures stored in the cell.
    pub attachments: Option<Value>,
    /// Every other field of the cell, kept as it was.
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notebook {
    pub nbformat: u64,
    pub nbformat_minor: u64,
    pub metadata: Map<String, Value>,
    pub cells: Vec<Cell>,
    /// Every other top-level field, kept as it was.
    pub extra: Map<String, Value>,
}

/// Why a file can't be opened as a notebook, in words for the person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// An nbformat 3 notebook (IPython 2), or older.
    OldFormat(u64),
    /// A notebook from a newer nbformat than 4.
    NewFormat(u64),
    /// Not JSON, or not shaped like a notebook.
    Broken(String),
}

impl Problem {
    pub fn words(&self) -> String {
        match self {
            Self::OldFormat(version) => crate::i18n::tf!("This notebook is in nbformat {version}, an old layout that Limoniter doesn't read as a notebook. You can still see and edit it as text. To bring it up to date, run: jupyter nbconvert --to notebook --nbformat 4 file.ipynb"),
            Self::NewFormat(version) => crate::i18n::tf!("This notebook is in nbformat {version}, which is newer than Limoniter knows. You can still see and edit it as text."),
            Self::Broken(why) => crate::i18n::tf!("This file isn't a notebook Limoniter can read: {why}"),
        }
    }
}

fn fresh_uid() -> u64 {
    NEXT_UID.fetch_add(1, Ordering::Relaxed)
}

/// An id like Jupyter's: eight hexadecimal digits. It only has to differ from the others in the notebook.
fn fresh_id(taken: &[Cell]) -> String {
    use std::hash::{BuildHasher, Hasher};
    loop {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(fresh_uid());
        let id = format!("{:08x}", hasher.finish() & 0xffff_ffff);
        if taken.iter().all(|cell| cell.id.as_deref() != Some(id.as_str())) {
            return id;
        }
    }
}

impl Cell {
    pub fn new(kind: CellKind, source: &str) -> Self {
        Self { uid: fresh_uid(), kind, id: None, source: source.to_string(), metadata: Map::new(), execution_count: None, outputs: Vec::new(), attachments: None, extra: Map::new() }
    }

    /// Whether the outputs are hidden, as JupyterLab records it (`metadata.jupyter.outputs_hidden`).
    pub fn outputs_hidden(&self) -> bool {
        self.metadata.get("jupyter").and_then(|j| j.get("outputs_hidden")).and_then(Value::as_bool).unwrap_or(false)
    }

    pub fn set_outputs_hidden(&mut self, hidden: bool) {
        if hidden {
            let jupyter = self.metadata.entry("jupyter").or_insert_with(|| Value::Object(Map::new()));
            if let Value::Object(map) = jupyter {
                map.insert("outputs_hidden".into(), Value::Bool(true));
            }
        } else if let Some(Value::Object(map)) = self.metadata.get_mut("jupyter") {
            map.remove("outputs_hidden");
            if map.is_empty() {
                self.metadata.remove("jupyter");
            }
        }
    }

    fn to_value(&self) -> Value {
        let mut map = self.extra.clone();
        map.insert("cell_type".into(), self.kind.json_name().into());
        map.insert("metadata".into(), Value::Object(self.metadata.clone()));
        map.insert("source".into(), self.source.clone().into());
        if let Some(id) = &self.id {
            map.insert("id".into(), id.clone().into());
        }
        if self.kind == CellKind::Code {
            map.insert("execution_count".into(), self.execution_count.map_or(Value::Null, Value::from));
            map.insert("outputs".into(), Value::Array(self.outputs.clone()));
        } else if let Some(attachments) = &self.attachments {
            map.insert("attachments".into(), attachments.clone());
        }
        Value::Object(map)
    }

    fn from_value(value: Value, number: usize) -> Result<Self, Problem> {
        let Value::Object(mut map) = value else {
            return Err(Problem::Broken(crate::i18n::tf!("cell {number} isn't an object")));
        };
        let kind = match map.remove("cell_type").as_ref().and_then(Value::as_str) {
            Some("code") => CellKind::Code,
            Some("markdown") => CellKind::Markdown,
            Some("raw") => CellKind::Raw,
            other => return Err(Problem::Broken(crate::i18n::tf!("cell {number} has the type {}", other.unwrap_or("(none)")))),
        };
        let source = match map.remove("source") {
            Some(Value::String(text)) => text,
            Some(Value::Array(lines)) => lines.iter().filter_map(Value::as_str).collect(),
            _ => String::new(),
        };
        let metadata = match map.remove("metadata") {
            Some(Value::Object(m)) => m,
            _ => Map::new(),
        };
        let id = match map.remove("id") {
            Some(Value::String(id)) => Some(id),
            Some(other) => {
                map.insert("id".into(), other);
                None
            }
            None => None,
        };
        let (mut execution_count, mut outputs, mut attachments) = (None, Vec::new(), None);
        if kind == CellKind::Code {
            execution_count = map.remove("execution_count").and_then(|c| c.as_u64());
            if let Some(Value::Array(list)) = map.remove("outputs") {
                outputs = list;
            }
        } else {
            attachments = map.remove("attachments");
        }
        Ok(Self { uid: fresh_uid(), kind, id, source, metadata, execution_count, outputs, attachments, extra: map })
    }

    /// Takes away what running left: the outputs and the count.
    pub fn clear_outputs(&mut self) {
        self.outputs.clear();
        self.execution_count = None;
    }
}

impl Notebook {
    /// A notebook with one empty code cell, for a kernel (a name like "python3" and what to call it) and its
    /// language.
    pub fn new(kernel_name: &str, display_name: &str, language: &str) -> Self {
        let mut metadata = Map::new();
        metadata.insert("kernelspec".into(), serde_json::json!({"display_name": display_name, "language": language, "name": kernel_name}));
        metadata.insert("language_info".into(), serde_json::json!({"name": language}));
        let mut notebook = Self { nbformat: 4, nbformat_minor: NEW_MINOR, metadata, cells: Vec::new(), extra: Map::new() };
        notebook.insert_cell(0, CellKind::Code);
        // The same every time, so File, New as's list of kinds doesn't look changed; ids need only differ
        // within one notebook.
        notebook.cells[0].id = Some("a1b2c3d4".into());
        notebook
    }

    pub fn parse(text: &str) -> Result<Self, Problem> {
        let value: Value = serde_json::from_str(text).map_err(|e| Problem::Broken(e.to_string()))?;
        let Value::Object(mut map) = value else {
            return Err(Problem::Broken(crate::i18n::tr("it isn't a JSON object").to_string()));
        };
        let version = map.remove("nbformat").and_then(|v| v.as_u64());
        match version {
            Some(4) => {}
            Some(other) if other < 4 => return Err(Problem::OldFormat(other)),
            Some(other) => return Err(Problem::NewFormat(other)),
            None if map.contains_key("worksheets") => return Err(Problem::OldFormat(3)),
            None => return Err(Problem::Broken(crate::i18n::tr("it has no nbformat number").to_string())),
        }
        let nbformat_minor = map.remove("nbformat_minor").and_then(|v| v.as_u64()).unwrap_or(0);
        let metadata = match map.remove("metadata") {
            Some(Value::Object(m)) => m,
            _ => Map::new(),
        };
        let Some(Value::Array(cells)) = map.remove("cells") else {
            return Err(Problem::Broken(crate::i18n::tr("it has no list of cells").to_string()));
        };
        let mut notebook = Self { nbformat: 4, nbformat_minor, metadata, cells: Vec::new(), extra: map };
        for (number, cell) in cells.into_iter().enumerate() {
            let mut cell = Cell::from_value(rejoin_cell(cell), number + 1)?;
            strip_cell(&mut cell);
            notebook.cells.push(cell);
        }
        strip_notebook(&mut notebook);
        Ok(notebook)
    }

    fn to_value(&self) -> Value {
        let mut map = self.extra.clone();
        map.insert("nbformat".into(), self.nbformat.into());
        map.insert("nbformat_minor".into(), self.nbformat_minor.into());
        map.insert("metadata".into(), Value::Object(self.metadata.clone()));
        map.insert("cells".into(), Value::Array(self.cells.iter().map(Cell::to_value).collect()));
        Value::Object(map)
    }

    /// The file's text, as Jupyter writes it: sorted keys, one space of indent, non-ASCII as it is, a newline at
    /// the end; long text split into lines.
    pub fn to_json(&self) -> String {
        let mut value = self.to_value();
        split_lines(&mut value);
        let mut out = String::new();
        write_value(&value, 0, &mut out);
        out.push('\n');
        out
    }

    /// The text's lines, as the editor holds a file: no line ends, and an empty last line after the final newline.
    pub fn to_lines(&self) -> Vec<String> {
        self.to_json().split('\n').map(str::to_string).collect()
    }

    /// The kernel's name from `metadata.kernelspec.name`.
    pub fn kernel_name(&self) -> Option<&str> {
        self.metadata.get("kernelspec")?.get("name")?.as_str()
    }

    /// The language, from `language_info.name`, else the kernelspec's.
    pub fn language(&self) -> Option<&str> {
        self.metadata.get("language_info").and_then(|l| l.get("name")).and_then(Value::as_str).or_else(|| self.metadata.get("kernelspec")?.get("language")?.as_str())
    }

    /// Records the kernel chosen, keeping whatever else `kernelspec` holds.
    pub fn set_kernel(&mut self, name: &str, display_name: &str, language: &str) {
        let spec = self.metadata.entry("kernelspec").or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(map) = spec {
            map.insert("name".into(), name.into());
            map.insert("display_name".into(), display_name.into());
            map.insert("language".into(), language.into());
        }
        let info = self.metadata.entry("language_info").or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(map) = info {
            // Another language's details (its version, its file ending) would be wrong now.
            if map.get("name").and_then(Value::as_str).is_some_and(|n| !n.eq_ignore_ascii_case(language)) {
                map.clear();
            }
            map.insert("name".into(), language.into());
        }
    }

    /// After the notebook has been read again (an undo, a file reloaded), each cell that is still the same one takes
    /// the number it had, by its `id` if the notebook has ids, else by its place, so that a cell that's running,
    /// selected or being edited stays so.
    pub fn keep_uids_from(&mut self, old: &Notebook) {
        for (at, cell) in self.cells.iter_mut().enumerate() {
            let same = match &cell.id {
                Some(id) => old.cells.iter().find(|o| o.id.as_ref() == Some(id)),
                None => old.cells.get(at).filter(|o| o.id.is_none() && o.kind == cell.kind),
            };
            if let Some(same) = same {
                cell.uid = same.uid;
            }
        }
    }

    pub fn index_of(&self, uid: u64) -> Option<usize> {
        self.cells.iter().position(|c| c.uid == uid)
    }

    fn make_cell(&self, kind: CellKind, source: &str) -> Cell {
        let mut cell = Cell::new(kind, source);
        if self.nbformat_minor >= 5 {
            cell.id = Some(fresh_id(&self.cells));
        }
        cell
    }

    /// Puts an empty cell in at a place (the end if past it), returning its place.
    pub fn insert_cell(&mut self, at: usize, kind: CellKind) -> usize {
        let at = at.min(self.cells.len());
        let cell = self.make_cell(kind, "");
        self.cells.insert(at, cell);
        at
    }

    /// Takes a cell out. A notebook is never left with none: the last one is emptied instead.
    pub fn delete_cell(&mut self, at: usize) -> Option<Cell> {
        if at >= self.cells.len() {
            return None;
        }
        if self.cells.len() == 1 {
            let kind = self.cells[0].kind;
            let fresh = self.make_cell(kind, "");
            return Some(std::mem::replace(&mut self.cells[0], fresh));
        }
        Some(self.cells.remove(at))
    }

    /// Moves a cell up or down by one, returning where it went.
    pub fn move_cell(&mut self, at: usize, down: bool) -> Option<usize> {
        let to = if down { at + 1 } else { at.checked_sub(1)? };
        if at >= self.cells.len() || to >= self.cells.len() {
            return None;
        }
        self.cells.swap(at, to);
        Some(to)
    }

    /// Splits a cell in two at a character's place in its source: the first keeps the outputs and the id. The
    /// new cell is the same kind, and comes after.
    pub fn split_cell(&mut self, at: usize, char_offset: usize) -> Option<usize> {
        let cell = self.cells.get(at)?;
        let byte = cell.source.char_indices().nth(char_offset).map_or(cell.source.len(), |(b, _)| b);
        let (first, second) = (cell.source[..byte].trim_end_matches(['\n', '\r']).to_string(), cell.source[byte..].trim_start_matches(['\n', '\r']).to_string());
        let kind = cell.kind;
        let fresh = self.make_cell(kind, &second);
        self.cells[at].source = first;
        self.cells.insert(at + 1, fresh);
        Some(at + 1)
    }

    /// Joins a cell with the one after it, the text on separate lines. The outputs of the second go.
    pub fn join_with_next(&mut self, at: usize) -> bool {
        if at + 1 >= self.cells.len() {
            return false;
        }
        let next = self.cells.remove(at + 1);
        let cell = &mut self.cells[at];
        if cell.source.is_empty() {
            cell.source = next.source;
        } else if !next.source.is_empty() {
            cell.source.push('\n');
            cell.source.push_str(&next.source);
        }
        true
    }

    /// Changes what a cell is. A code cell made into text loses its outputs and its count; text made into code
    /// has none yet.
    pub fn set_kind(&mut self, at: usize, kind: CellKind) -> bool {
        let Some(cell) = self.cells.get_mut(at) else {
            return false;
        };
        if cell.kind == kind {
            return false;
        }
        if kind == CellKind::Code {
            cell.attachments = None;
        } else {
            cell.outputs.clear();
            cell.execution_count = None;
        }
        cell.kind = kind;
        true
    }

    pub fn clear_all_outputs(&mut self) -> bool {
        let mut changed = false;
        for cell in &mut self.cells {
            if cell.kind == CellKind::Code && (!cell.outputs.is_empty() || cell.execution_count.is_some()) {
                cell.clear_outputs();
                changed = true;
            }
        }
        changed
    }
}

// ---- the file's layout, as nbformat has it ----

/// Joins what was saved as lists of lines back into text (nbformat's `rejoin_lines`).
fn rejoin_cell(mut cell: Value) -> Value {
    let Some(map) = cell.as_object_mut() else {
        return cell;
    };
    if let Some(Value::Array(lines)) = map.get("source")
        && lines.iter().all(Value::is_string)
    {
        let joined: String = lines.iter().filter_map(Value::as_str).collect();
        map.insert("source".into(), joined.into());
    }
    if let Some(Value::Object(attachments)) = map.get_mut("attachments") {
        for bundle in attachments.values_mut() {
            rejoin_bundle(bundle);
        }
    }
    if map.get("cell_type").and_then(Value::as_str) == Some("code")
        && let Some(Value::Array(outputs)) = map.get_mut("outputs")
    {
        for output in outputs {
            let Some(out) = output.as_object_mut() else { continue };
            match out.get("output_type").and_then(Value::as_str) {
                Some("execute_result" | "display_data") => {
                    if let Some(data) = out.get_mut("data") {
                        rejoin_bundle(data);
                    }
                }
                Some(_) => {
                    if let Some(Value::Array(lines)) = out.get("text")
                        && lines.iter().all(Value::is_string)
                    {
                        let joined: String = lines.iter().filter_map(Value::as_str).collect();
                        out.insert("text".into(), joined.into());
                    }
                }
                None => {}
            }
        }
    }
    cell
}

fn is_json_mime(mime: &str) -> bool {
    mime == "application/json" || (mime.starts_with("application/") && mime.ends_with("+json"))
}

fn rejoin_bundle(bundle: &mut Value) {
    let Some(map) = bundle.as_object_mut() else { return };
    for (mime, value) in map.iter_mut() {
        if is_json_mime(mime) {
            continue;
        }
        if let Value::Array(lines) = value
            && lines.iter().all(Value::is_string)
        {
            *value = Value::String(lines.iter().filter_map(Value::as_str).collect());
        }
    }
}

/// What Jupyter never keeps in a file: the notebook's `orig_nbformat`, `orig_nbformat_minor` and `signature`, and
/// each cell's `trusted`.
fn strip_notebook(notebook: &mut Notebook) {
    for key in ["orig_nbformat", "orig_nbformat_minor", "signature"] {
        notebook.metadata.remove(key);
    }
}

fn strip_cell(cell: &mut Cell) {
    cell.metadata.remove("trusted");
}

/// Python's `str.splitlines(True)`: the pieces keep their line ends, and what ends a line is more than `\n`.
pub fn split_keeping_ends(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let mut end = i + c.len_utf8();
        match c {
            '\r' => {
                if let Some(&(_, '\n')) = chars.peek() {
                    chars.next();
                    end += 1;
                }
            }
            '\n' | '\u{b}' | '\u{c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}' => {}
            _ => continue,
        }
        pieces.push(&text[start..end]);
        start = end;
    }
    if start < text.len() {
        pieces.push(&text[start..]);
    }
    pieces
}

fn lines_value(text: &str) -> Value {
    Value::Array(split_keeping_ends(text).into_iter().map(|l| Value::String(l.to_string())).collect())
}

fn split_bundle(bundle: &mut Value) {
    let Some(map) = bundle.as_object_mut() else { return };
    for (mime, value) in map.iter_mut() {
        if let Value::String(text) = value
            && (mime.starts_with("text/") || mime == "application/javascript" || mime == "image/svg+xml")
        {
            *value = lines_value(text);
        }
    }
}

/// nbformat's `split_lines`, applied to the whole notebook on its way to a file.
fn split_lines(notebook: &mut Value) {
    let Some(Value::Array(cells)) = notebook.get_mut("cells") else { return };
    for cell in cells {
        let Some(map) = cell.as_object_mut() else { continue };
        if let Some(Value::String(source)) = map.get("source") {
            let lines = lines_value(source);
            map.insert("source".into(), lines);
        }
        if let Some(Value::Object(attachments)) = map.get_mut("attachments") {
            for bundle in attachments.values_mut() {
                split_bundle(bundle);
            }
        }
        if map.get("cell_type").and_then(Value::as_str) == Some("code")
            && let Some(Value::Array(outputs)) = map.get_mut("outputs")
        {
            for output in outputs {
                let Some(out) = output.as_object_mut() else { continue };
                match out.get("output_type").and_then(Value::as_str) {
                    Some("execute_result" | "display_data") => {
                        if let Some(data) = out.get_mut("data") {
                            split_bundle(data);
                        }
                        // What the kernel sent to update a display later isn't part of the notebook.
                        out.remove("transient");
                    }
                    Some("stream") => {
                        if let Some(Value::String(text)) = out.get("text") {
                            let lines = lines_value(text);
                            out.insert("text".into(), lines);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

// ---- JSON as Python's json.dumps(indent=1, sort_keys=True, ensure_ascii=False) writes it ----

fn write_value(value: &Value, depth: usize, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&python_number(n)),
        Value::String(s) => write_string(s, out),
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('\n');
                out.extend(std::iter::repeat_n(' ', depth + 1));
                write_value(item, depth + 1, out);
            }
            out.push('\n');
            out.extend(std::iter::repeat_n(' ', depth));
            out.push(']');
        }
        Value::Object(map) if map.is_empty() => out.push_str("{}"),
        Value::Object(map) => {
            // Python sorts keys by their characters, which is the order of their UTF-8 bytes too.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('\n');
                out.extend(std::iter::repeat_n(' ', depth + 1));
                write_string(key, out);
                out.push_str(": ");
                write_value(&map[key], depth + 1, out);
            }
            out.push('\n');
            out.extend(std::iter::repeat_n(' ', depth));
            out.push('}');
        }
    }
}

fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A number as Python prints it: `1e-05`, `1e+16`, `100.0`.
fn python_number(n: &serde_json::Number) -> String {
    if n.is_i64() || n.is_u64() {
        return n.to_string();
    }
    let f = n.as_f64().unwrap_or(0.0);
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    // Rust's shortest digits, "d.ddde-5", are the ones Python picks too; only the layout differs.
    let text = format!("{:e}", f.abs());
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let sign = if f < 0.0 { "-" } else { "" };
    if (-4..16).contains(&exponent) {
        let count = digits.len() as i32;
        let body = if exponent >= count - 1 {
            format!("{digits}{}.0", "0".repeat((exponent - (count - 1)) as usize))
        } else if exponent >= 0 {
            format!("{}.{}", &digits[..=exponent as usize], &digits[exponent as usize + 1..])
        } else {
            format!("0.{}{digits}", "0".repeat((-exponent - 1) as usize))
        };
        format!("{sign}{body}")
    } else {
        let mantissa = if digits.len() > 1 { format!("{}.{}", &digits[..1], &digits[1..]) } else { digits };
        format!("{sign}{mantissa}e{}{:02}", if exponent < 0 { '-' } else { '+' }, exponent.abs())
    }
}

// ---- comparing notebooks cell by cell ----

/// A notebook as lines of text for a comparison: a marker line for each cell, then its source. With `outputs`, each
/// code cell's count and the plain text of its outputs come too (pictures as a word). If the text isn't a notebook
/// it comes back as its own lines.
pub fn cells_as_lines(json: &str, outputs: bool) -> Vec<String> {
    let Ok(notebook) = Notebook::parse(json) else {
        return json.lines().map(str::to_string).collect();
    };
    let mut lines = Vec::new();
    for cell in &notebook.cells {
        let count = match (outputs, cell.kind, cell.execution_count) {
            (true, CellKind::Code, Some(n)) => format!(" [{n}]"),
            _ => String::new(),
        };
        lines.push(format!("# %% [{}]{count}", cell.kind.json_name()));
        lines.extend(cell.source.split('\n').map(str::to_string));
        if outputs {
            for output in &cell.outputs {
                for line in crate::notebook_output::plain_text_of(output).lines() {
                    lines.push(format!("#   | {line}"));
                }
            }
        }
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILES: [(&str, &str); 6] = [
        ("outputs", include_str!("notebook_testdata/outputs.ipynb")),
        ("pandas", include_str!("notebook_testdata/pandas.ipynb")),
        ("unicode", include_str!("notebook_testdata/unicode.ipynb")),
        ("old", include_str!("notebook_testdata/old.ipynb")),
        ("r", include_str!("notebook_testdata/r.ipynb")),
        ("empty", include_str!("notebook_testdata/empty.ipynb")),
    ];

    #[test]
    fn notebooks_saved_by_nbformat_come_out_byte_for_byte() {
        for (name, text) in FILES {
            let notebook = Notebook::parse(text).unwrap_or_else(|p| panic!("{name}: {p:?}"));
            let saved = notebook.to_json();
            if saved != text {
                let at = saved.bytes().zip(text.bytes()).position(|(a, b)| a != b).unwrap_or(saved.len().min(text.len()));
                panic!("{name} differs at byte {at}: {:?} against {:?}", &saved[at.saturating_sub(30)..(at + 30).min(saved.len())], &text[at.saturating_sub(30)..(at + 30).min(text.len())]);
            }
        }
    }

    #[test]
    fn what_is_read_is_the_cells_and_their_fields() {
        let notebook = Notebook::parse(FILES[0].1).unwrap();
        assert_eq!(notebook.nbformat_minor, 5);
        assert_eq!(notebook.cells.len(), 6);
        assert_eq!(notebook.cells[0].kind, CellKind::Markdown);
        assert_eq!(notebook.cells[0].source, "# Title\n\nSome *text* with `code`.\n");
        assert_eq!(notebook.cells[1].execution_count, Some(1));
        assert_eq!(notebook.cells[1].source, "print(1)\n1+1\nimport sys; sys.stderr.write('e')");
        assert_eq!(notebook.cells[1].outputs.len(), 3);
        assert_eq!(notebook.cells[4].kind, CellKind::Raw);
        assert!(notebook.cells[0].id.is_some());
        assert_eq!(notebook.kernel_name(), Some("python3"));
        assert_eq!(notebook.language(), Some("python"));
        let r = Notebook::parse(FILES[4].1).unwrap();
        assert_eq!((r.kernel_name(), r.language()), (Some("ir"), Some("R")));
    }

    #[test]
    fn a_notebook_of_nbformat_4_4_keeps_having_no_ids_and_its_attachments() {
        let mut notebook = Notebook::parse(FILES[3].1).unwrap();
        assert_eq!(notebook.nbformat_minor, 4);
        assert!(notebook.cells.iter().all(|c| c.id.is_none()));
        assert!(notebook.cells[0].attachments.is_some());
        notebook.insert_cell(2, CellKind::Code);
        assert!(notebook.cells[2].id.is_none(), "a 4.4 notebook has no ids, and a new cell doesn't get one");
        let mut fresh = Notebook::parse(FILES[0].1).unwrap();
        fresh.insert_cell(1, CellKind::Markdown);
        let id = fresh.cells[1].id.clone().expect("4.5 cells have ids");
        assert_eq!(id.len(), 8);
        assert_eq!(fresh.cells.iter().filter(|c| c.id.as_deref() == Some(id.as_str())).count(), 1);
    }

    #[test]
    fn fields_nobody_here_knows_are_kept() {
        let text = r#"{"cells":[{"cell_type":"code","execution_count":null,"id":"a1","metadata":{"x":[1]},"outputs":[{"output_type":"stream","name":"stdout","text":"hi","extra_thing":1}],"source":"1","future_field":{"k":"v"}}],"metadata":{"mystery":true},"nbformat":4,"nbformat_minor":5,"top_level_extra":3}"#;
        let notebook = Notebook::parse(text).unwrap();
        let saved = notebook.to_json();
        for part in ["\"future_field\": {", "\"mystery\": true", "\"top_level_extra\": 3", "\"extra_thing\": 1", "\"x\": ["] {
            assert!(saved.contains(part), "{part} lost from {saved}");
        }
        assert_eq!(Notebook::parse(&saved).unwrap().to_json(), saved);
    }

    #[test]
    fn nbformat_3_is_said_not_to_be_supported_and_so_is_junk() {
        let old = r#"{"nbformat": 3, "nbformat_minor": 0, "worksheets": [{"cells": []}], "metadata": {}}"#;
        assert_eq!(Notebook::parse(old), Err(Problem::OldFormat(3)));
        let older = r#"{"worksheets": [], "metadata": {}}"#;
        assert_eq!(Notebook::parse(older), Err(Problem::OldFormat(3)));
        assert_eq!(Notebook::parse(r#"{"nbformat": 5, "cells": []}"#), Err(Problem::NewFormat(5)));
        assert!(matches!(Notebook::parse("not json"), Err(Problem::Broken(_))));
        assert!(matches!(Notebook::parse("[1]"), Err(Problem::Broken(_))));
        assert!(matches!(Notebook::parse(r#"{"nbformat": 4, "cells": [{"cell_type": "weird"}]}"#), Err(Problem::Broken(_))));
        assert!(Problem::OldFormat(3).words().contains("nbformat 3"));
    }

    #[test]
    fn lists_of_lines_are_joined_and_split_the_way_python_does() {
        assert_eq!(split_keeping_ends("a\nb\r\nc\rd\u{2028}e"), vec!["a\n", "b\r\n", "c\r", "d\u{2028}", "e"]);
        assert_eq!(split_keeping_ends("a\n\n"), vec!["a\n", "\n"]);
        assert!(split_keeping_ends("").is_empty());
        // A file that had its lines split somewhere else is saved split at line ends.
        let text = r##"{"cells":[{"cell_type":"markdown","metadata":{},"source":["# a","b\n","c"]}],"metadata":{},"nbformat":4,"nbformat_minor":4}"##;
        let notebook = Notebook::parse(text).unwrap();
        assert_eq!(notebook.cells[0].source, "# ab\nc");
        assert!(notebook.to_json().contains("\"# ab\\n\",\n    \"c\""));
    }

    #[test]
    fn numbers_are_written_as_python_writes_them() {
        let n = |f: f64| python_number(&serde_json::Number::from_f64(f).unwrap());
        assert_eq!(n(1e-5), "1e-05");
        assert_eq!(n(1e16), "1e+16");
        assert_eq!(n(1e15), "1000000000000000.0");
        assert_eq!(n(100.0), "100.0");
        assert_eq!(n(0.1), "0.1");
        assert_eq!(n(-2.5), "-2.5");
        assert_eq!(n(0.0001), "0.0001");
        assert_eq!(n(1.5e300), "1.5e+300");
        assert_eq!(n(123456.789), "123456.789");
        assert_eq!(n(1.7976931348623157e308), "1.7976931348623157e+308");
    }

    #[test]
    fn transient_fields_are_dropped_as_jupyter_drops_them() {
        let text = r#"{"cells":[{"cell_type":"code","execution_count":1,"metadata":{"trusted":true,"keep":1},"outputs":[{"output_type":"display_data","data":{"text/plain":"x"},"metadata":{},"transient":{"display_id":"d1"}}],"source":""}],"metadata":{"orig_nbformat":4,"signature":"x","a":1},"nbformat":4,"nbformat_minor":4}"#;
        let notebook = Notebook::parse(text).unwrap();
        assert!(!notebook.metadata.contains_key("orig_nbformat") && !notebook.metadata.contains_key("signature"));
        assert!(!notebook.cells[0].metadata.contains_key("trusted") && notebook.cells[0].metadata.contains_key("keep"));
        let saved = notebook.to_json();
        assert!(!saved.contains("transient") && !saved.contains("display_id"));
    }

    #[test]
    fn a_new_notebook_is_valid_and_reads_back() {
        let notebook = Notebook::new("python3", "Python 3", "python");
        let json = notebook.to_json();
        assert!(json.starts_with("{\n \"cells\": [\n  {\n   \"cell_type\": \"code\",\n   \"execution_count\": null,"));
        assert!(json.ends_with("}\n"));
        let back = Notebook::parse(&json).unwrap();
        assert_eq!(back.cells.len(), 1);
        assert_eq!(back.kernel_name(), Some("python3"));
        assert_eq!(back.to_json(), json);
        assert_eq!(notebook.to_lines().last().map(String::as_str), Some(""));
    }

    #[test]
    fn cells_are_added_deleted_and_moved() {
        let mut nb = Notebook::new("python3", "Python 3", "python");
        nb.cells[0].source = "a".into();
        assert_eq!(nb.insert_cell(1, CellKind::Markdown), 1);
        nb.cells[1].source = "b".into();
        assert_eq!(nb.insert_cell(0, CellKind::Code), 0);
        nb.cells[0].source = "c".into();
        let order = |nb: &Notebook| nb.cells.iter().map(|c| c.source.clone()).collect::<Vec<_>>().join("");
        assert_eq!(order(&nb), "cab");
        assert_eq!(nb.move_cell(0, true), Some(1));
        assert_eq!(order(&nb), "acb");
        assert_eq!(nb.move_cell(2, true), None);
        assert_eq!(nb.move_cell(0, false), None);
        assert_eq!(nb.move_cell(2, false), Some(1));
        assert_eq!(order(&nb), "abc");
        assert_eq!(nb.delete_cell(1).map(|c| c.source), Some("b".to_string()));
        assert_eq!(order(&nb), "ac");
        nb.delete_cell(0);
        nb.delete_cell(0);
        assert_eq!(nb.cells.len(), 1, "the last cell is emptied, not removed");
        assert_eq!(nb.cells[0].source, "");
    }

    #[test]
    fn a_cell_splits_and_joins() {
        let mut nb = Notebook::new("python3", "Python 3", "python");
        nb.cells[0].source = "x = 1\ny = 2".into();
        nb.cells[0].execution_count = Some(4);
        nb.cells[0].outputs.push(serde_json::json!({"output_type": "stream", "name": "stdout", "text": "o"}));
        // After the first line's end.
        assert_eq!(nb.split_cell(0, 5), Some(1));
        assert_eq!(nb.cells[0].source, "x = 1");
        assert_eq!(nb.cells[1].source, "y = 2");
        assert_eq!(nb.cells[0].outputs.len(), 1, "the first half keeps the outputs");
        assert!(nb.cells[1].outputs.is_empty() && nb.cells[1].execution_count.is_none());
        assert_ne!(nb.cells[0].id, nb.cells[1].id);
        assert!(nb.join_with_next(0));
        assert_eq!(nb.cells[0].source, "x = 1\ny = 2");
        assert_eq!(nb.cells.len(), 1);
        assert!(!nb.join_with_next(0));
        // Multi-byte characters are counted as characters.
        nb.cells[0].source = "h\u{e9}llo wor".into();
        nb.split_cell(0, 6);
        assert_eq!((nb.cells[0].source.as_str(), nb.cells[1].source.as_str()), ("h\u{e9}llo ", "wor"));
    }

    #[test]
    fn changing_a_cells_kind_drops_what_no_longer_applies() {
        let mut nb = Notebook::new("python3", "Python 3", "python");
        nb.cells[0].source = "1".into();
        nb.cells[0].execution_count = Some(2);
        nb.cells[0].outputs.push(serde_json::json!({"output_type": "stream", "name": "stdout", "text": "o"}));
        assert!(nb.set_kind(0, CellKind::Markdown));
        assert!(nb.cells[0].outputs.is_empty() && nb.cells[0].execution_count.is_none());
        let json = nb.to_json();
        assert!(!json.contains("execution_count") && !json.contains("\"outputs\""), "{json}");
        assert!(!nb.set_kind(0, CellKind::Markdown));
        assert!(nb.set_kind(0, CellKind::Code));
        assert!(nb.to_json().contains("\"outputs\": []"));
    }

    #[test]
    fn outputs_are_cleared_and_hidden() {
        let mut nb = Notebook::parse(FILES[0].1).unwrap();
        assert!(nb.clear_all_outputs());
        assert!(nb.cells.iter().all(|c| c.outputs.is_empty() && c.execution_count.is_none()));
        assert!(!nb.clear_all_outputs());
        let cell = &mut nb.cells[1];
        assert!(!cell.outputs_hidden());
        cell.set_outputs_hidden(true);
        assert!(cell.outputs_hidden());
        assert!(nb.to_json().contains("\"outputs_hidden\": true"));
        nb.cells[1].set_outputs_hidden(false);
        assert!(!nb.cells[1].metadata.contains_key("jupyter"));
    }

    #[test]
    fn choosing_a_kernel_keeps_the_rest_of_its_record() {
        let mut nb = Notebook::parse(FILES[0].1).unwrap();
        nb.set_kernel("ir", "R", "R");
        assert_eq!((nb.kernel_name(), nb.language()), (Some("ir"), Some("R")));
        let info = nb.metadata["language_info"].as_object().unwrap();
        assert_eq!(info.len(), 1, "Python's version and file ending don't stay: {info:?}");
    }

    #[test]
    fn cells_keep_their_numbers_when_the_notebook_is_read_again() {
        let old = Notebook::parse(FILES[0].1).unwrap();
        let mut again = Notebook::parse(FILES[0].1).unwrap();
        assert_ne!(again.cells[1].uid, old.cells[1].uid);
        again.keep_uids_from(&old);
        assert!(again.cells.iter().zip(&old.cells).all(|(a, b)| a.uid == b.uid));
        // Without ids, by place; a cell that's changed kind is a different one.
        let old = Notebook::parse(FILES[3].1).unwrap();
        let mut again = Notebook::parse(FILES[3].1).unwrap();
        again.cells[1].kind = CellKind::Raw;
        again.keep_uids_from(&old);
        assert_eq!(again.cells[0].uid, old.cells[0].uid);
        assert_ne!(again.cells[1].uid, old.cells[1].uid);
    }

    #[test]
    fn a_comparison_shows_sources_and_leaves_outputs_out_unless_asked() {
        let lines = cells_as_lines(FILES[0].1, false);
        assert_eq!(lines[0], "# %% [markdown]");
        assert!(lines.contains(&"print(1)".to_string()));
        assert!(!lines.iter().any(|l| l.contains("#   |")) && !lines.iter().any(|l| l.contains("[1]")));
        let with = cells_as_lines(FILES[0].1, true);
        assert!(with.contains(&"# %% [code] [1]".to_string()));
        assert!(with.contains(&"#   | 2".to_string()), "{with:?}");
        assert!(with.iter().any(|l| l.contains("ZeroDivisionError")));
        // A file that isn't a notebook is compared as it is.
        assert_eq!(cells_as_lines("a\nb", false), vec!["a", "b"]);
    }
}
