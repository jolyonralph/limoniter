//! The NDK's assembler headers (Include_I), read for what their names are: each structure's fields, their offsets
//! and sizes, each structure's size, and the flags and other values defined there. A structure starts with
//! STRUCTURE name,offset (the offset being where a structure it builds on ends, such as LN_SIZE), and each line
//! after it gives a field of a size (UBYTE, UWORD, ULONG, APTR, STRUCT name,size and the rest), named after the
//! offset it's at; LABEL names the offset without moving on, which is how a structure's size is named. BITDEF
//! gives a bit's number and its mask. This reads the headers itself rather than assembling them, which is quick
//! and keeps what kind each field is.

use crate::asm::expr::{self, Scope, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// What a name in the headers is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum What {
    /// A field of a structure: its kind (APTR, UWORD, STRUCT) and size in bytes.
    Field { structure: String, kind: String, size: i64 },
    /// A structure's size, named by a LABEL at its end.
    Size { structure: String },
    /// A bit's number, and the name of its mask.
    Bit { mask: String },
    /// A bit's mask, and the name of its number.
    Mask { bit: String },
    /// Any other value, such as a node type.
    Value,
}

/// A name in the headers: its value, what it is, and the header it's in ("exec/nodes.i").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    pub value: i64,
    pub what: What,
    pub header: String,
}

/// The size of each kind of field types.i defines (and dos.i's BPTR and BSTR); STRUCT's is given with it.
fn field_size(kind: &str) -> Option<i64> {
    Some(match kind {
        "BYTE" | "UBYTE" => 1,
        "WORD" | "UWORD" | "BOOL" | "SHORT" | "USHORT" | "RPTR" => 2,
        "LONG" | "ULONG" | "APTR" | "CPTR" | "FPTR" | "FLOAT" | "BPTR" | "BSTR" => 4,
        "DOUBLE" => 8,
        "LABEL" => 0,
        _ => return None,
    })
}

struct Known<'a>(&'a HashMap<String, i64>);

impl Scope for Known<'_> {
    fn value(&self, name: &str) -> Option<i64> {
        self.0.get(name).copied()
    }
    fn here(&self) -> i64 {
        0
    }
    fn full_name(&self, name: &str) -> String {
        name.to_string()
    }
}

/// The operands of a line, up to the space that starts a comment, split at commas; <...> keeps one whole.
fn operands(text: &str) -> Vec<String> {
    let mut items = vec![String::new()];
    let (mut quote, mut depth) = (None, 0);
    for c in text.trim().chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '\'' | '"' => quote = Some(c),
                '(' | '<' => depth += 1,
                ')' | '>' if depth > 0 => depth -= 1,
                ' ' | '\t' if depth == 0 => break,
                ',' if depth == 0 => {
                    items.push(String::new());
                    continue;
                }
                _ => {}
            },
        }
        items.last_mut().expect("one at least").push(c);
    }
    items.into_iter().map(|i| i.trim().trim_start_matches('<').trim_end_matches('>').to_string()).filter(|i| !i.is_empty()).collect()
}

/// Reading the headers, one file at a time, each included one first.
struct Reader<'a> {
    root: &'a Path,
    done: HashSet<String>,
    values: HashMap<String, i64>,
    names: BTreeMap<String, Name>,
    /// The structure being read, and the offset of the next field.
    structure: Option<String>,
    offset: i64,
    /// What a LABEL has named in the structure so far, to tell its size once it ends.
    labels: Vec<String>,
    /// ENUM's next value, and DEVCMD's.
    item: i64,
    command: i64,
    /// LIBDEF's next.
    vector: i64,
}

impl Reader<'_> {
    fn eval(&self, text: &str) -> Option<i64> {
        match expr::eval(text, &Known(&self.values)) {
            Ok(Value::Known(v)) => Some(v),
            _ => None,
        }
    }

    fn define(&mut self, name: &str, value: i64, what: What, header: &str) {
        self.values.insert(name.to_string(), value);
        self.names.entry(name.to_string()).or_insert(Name { value, what, header: header.to_string() });
    }

    /// The structure being read has ended: a LABEL at its very end names its size.
    fn end_structure(&mut self) {
        if let Some(structure) = self.structure.take() {
            for label in std::mem::take(&mut self.labels) {
                if let Some(name) = self.names.get_mut(&label)
                    && name.value == self.offset
                    && matches!(&name.what, What::Field { structure: s, .. } if *s == structure)
                {
                    name.what = What::Size { structure: structure.clone() };
                }
            }
        }
    }

    fn file(&mut self, header: &str) {
        if !self.done.insert(header.to_string()) {
            return;
        }
        let Some(text) = super::read_text(&self.root.join(header)) else {
            return;
        };
        let mut in_macro = false;
        let mut conditions: Vec<bool> = Vec::new();
        for line in text.lines() {
            let code = crate::asm::strip_comment(line);
            if code.trim_start().starts_with('*') || code.trim().is_empty() {
                continue;
            }
            // A label starts the line (or ends with a colon); then the operation and its operands.
            let (label, rest) = if code.starts_with([' ', '\t']) {
                (None, code.trim_start())
            } else {
                let end = code.find(char::is_whitespace).unwrap_or(code.len());
                (Some(code[..end].trim_end_matches(':')), code[end..].trim_start())
            };
            let (op, rest) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let upper = op.to_ascii_uppercase();
            // Macros' bodies are their users' business, not values.
            if upper == "MACRO" {
                in_macro = true;
                continue;
            }
            if in_macro {
                in_macro = upper != "ENDM";
                continue;
            }
            let args = operands(rest);
            let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
            // IFD and IFND choose by what's defined so far (such as NO_BIG_BLITS), the others by a value; one that
            // can't be worked out is taken as true.
            match upper.as_str() {
                "IFD" | "IFND" => {
                    let on = self.values.contains_key(arg(0)) == (upper == "IFD");
                    conditions.push(on);
                    continue;
                }
                "IF" | "IFEQ" | "IFNE" | "IFGT" | "IFGE" | "IFLT" | "IFLE" => {
                    let on = self.eval(arg(0)).is_none_or(|v| match upper.as_str() {
                        "IFEQ" => v == 0,
                        "IFGT" => v > 0,
                        "IFGE" => v >= 0,
                        "IFLT" => v < 0,
                        "IFLE" => v <= 0,
                        _ => v != 0,
                    });
                    conditions.push(on);
                    continue;
                }
                "IFC" | "IFNC" => {
                    conditions.push((arg(0) == arg(1)) == (upper == "IFC"));
                    continue;
                }
                "ELSE" => {
                    if let Some(on) = conditions.last_mut() {
                        *on = !*on;
                    }
                    continue;
                }
                "ENDC" | "ENDIF" => {
                    conditions.pop();
                    continue;
                }
                _ if !conditions.iter().all(|on| *on) => continue,
                _ => {}
            }
            match (label, upper.as_str()) {
                (Some(name), "EQU" | "SET" | "=") => {
                    if let Some(value) = self.eval(arg(0)) {
                        match name {
                            "SOFFSET" => self.offset = value,
                            "EOFFSET" => self.item = value,
                            "CMD_COUNT" => self.command = value,
                            // What SET gives can change (a counter, or a header saying it's been read): not a name to show.
                            _ if upper == "SET" => {
                                self.values.insert(name.to_string(), value);
                            }
                            _ => self.define(name, value, What::Value, header),
                        }
                    }
                }
                (_, "INCLUDE") => {
                    let included = arg(0).trim_matches(['"', '\'']).to_string();
                    self.file(&included);
                }
                (_, "STRUCTURE") => {
                    self.end_structure();
                    self.offset = self.eval(arg(1)).unwrap_or(0);
                    self.define(arg(0), 0, What::Value, header);
                    self.structure = Some(arg(0).to_string());
                }
                (_, "ALIGNWORD") => self.offset = (self.offset + 1) & !1,
                (_, "ALIGNLONG") => self.offset = (self.offset + 3) & !3,
                (_, "ENUM") => self.item = self.eval(arg(0)).unwrap_or(0),
                (_, "EITEM") => {
                    let item = self.item;
                    self.define(arg(0), item, What::Value, header);
                    self.item += 1;
                }
                (_, "DEVINIT") => self.command = self.eval(arg(0)).or_else(|| self.values.get("CMD_NONSTD").copied()).unwrap_or(0),
                // LIBDEF names a library's vectors, each six bytes below the one before, from LIBINIT's.
                (_, "LIBINIT") => self.vector = self.eval(arg(0)).or_else(|| self.values.get("LIB_USERDEF").copied()).unwrap_or(0),
                (_, "LIBDEF") => {
                    let vector = self.vector;
                    self.define(arg(0), vector, What::Value, header);
                    self.vector -= 6;
                }
                (_, "DEVCMD") => {
                    let command = self.command;
                    self.define(arg(0), command, What::Value, header);
                    self.command += 1;
                }
                (_, "BITDEF") => {
                    let Some(bit) = self.eval(arg(2)) else { continue };
                    let (number, mask) = (format!("{}B_{}", arg(0), arg(1)), format!("{}F_{}", arg(0), arg(1)));
                    self.define(&number, bit, What::Bit { mask: mask.clone() }, header);
                    self.define(&mask, 1i64.wrapping_shl(bit as u32), What::Mask { bit: number }, header);
                }
                (_, kind) => {
                    let size = match kind {
                        "STRUCT" => self.eval(arg(1)),
                        _ => field_size(kind),
                    };
                    if let (Some(size), Some(structure)) = (size, self.structure.clone())
                        && !arg(0).is_empty()
                    {
                        let offset = self.offset;
                        self.define(arg(0), offset, What::Field { structure, kind: kind.to_string(), size }, header);
                        if kind == "LABEL" {
                            self.labels.push(arg(0).to_string());
                        }
                        self.offset += size;
                    }
                }
            }
        }
        self.end_structure();
    }
}

/// Reads every header in an Include_I folder: the names they define, by name.
pub fn read(root: &Path) -> BTreeMap<String, Name> {
    let mut headers = Vec::new();
    for dir in std::fs::read_dir(root).into_iter().flatten().flatten().filter(|e| e.path().is_dir()) {
        for file in std::fs::read_dir(dir.path()).into_iter().flatten().flatten() {
            let name = file.file_name().to_string_lossy().into_owned();
            if name.to_ascii_lowercase().ends_with(".i") {
                headers.push(format!("{}/{name}", dir.file_name().to_string_lossy()));
            }
        }
    }
    headers.sort();
    // The _LVO files say only what the SFD files do.
    headers.retain(|h| !h.starts_with("lvo/"));
    let mut reader = Reader { root, done: HashSet::new(), values: HashMap::new(), names: BTreeMap::new(), structure: None, offset: 0, labels: Vec::new(), item: 0, command: 0, vector: 0 };
    for header in headers {
        reader.file(&header);
    }
    reader.names
}

impl Name {
    /// What to show for it: "LN_NAME: offset 10 in LN (APTR, 4 bytes)", and the header it's from.
    pub fn describe(&self, name: &str) -> String {
        let value = self.value;
        let line = match &self.what {
            What::Field { structure, kind, .. } if kind == "LABEL" => format!("{name}: offset {value} in {structure}"),
            What::Field { structure, kind, size } => if *size == 1 { crate::i18n::tf!("{name}: offset {value} in {structure} ({kind}, 1 byte)") } else { crate::i18n::tf!("{name}: offset {value} in {structure} ({kind}, {size} bytes)") },
            What::Size { structure } => crate::i18n::tf!("{name}: the size of {structure}, {value} bytes"),
            What::Bit { mask } => crate::i18n::tf!("{name} = {value}: a bit's number; its mask is {mask}"),
            What::Mask { bit } => format!("{name} = {value} (${:X}): bit {} ({bit})", value as u32, (value as u64).trailing_zeros()),
            What::Value if value < 0 => format!("{name} = {value}"),
            What::Value => format!("{name} = {value} (${:X})", value as u32),
        };
        format!("{line}\n{}", self.header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TYPES: &str = "\tIFND EXEC_TYPES_I\nEXEC_TYPES_I SET 1\nSTRUCTURE MACRO\n\\1 EQU 0\nSOFFSET SET \\2\n ENDM\n\tENDC\n";
    const NODES: &str = "    IFND EXEC_TYPES_I\n    INCLUDE \"exec/types.i\"\n    ENDC\n   STRUCTURE\tLN,0\t; List Node\n\tAPTR\tLN_SUCC\t; next\n\tAPTR\tLN_PRED\n\tUBYTE\tLN_TYPE\n\tBYTE\tLN_PRI\n\tAPTR\tLN_NAME\n\tLABEL\tLN_SIZE\t; Note: word aligned\nNT_TASK\tEQU 1\t; Exec task\n";
    const PORTS: &str = "\tINCLUDE \"exec/nodes.i\"\n STRUCTURE MP,LN_SIZE\n\tUBYTE MP_FLAGS\n\tUBYTE MP_SIGBIT\n\tAPTR MP_SIGTASK\n\tLABEL MP_MSGLIST\n\tSTRUCT MP_LIST,14 * a List\n\tALIGNLONG\n\tLABEL MP_SIZE\n\tBITDEF MEM,CHIP,1\nMODE_OLDFILE EQU 1005 * Open existing file\n ENUM 4\n EITEM FIRST\n EITEM SECOND\n";

    #[test]
    fn structures_fields_and_flags_are_read_from_headers() {
        let root = std::env::temp_dir().join(format!("limoniter-headers-{}", std::process::id()));
        std::fs::create_dir_all(root.join("exec")).unwrap();
        std::fs::write(root.join("exec/types.i"), TYPES).unwrap();
        std::fs::write(root.join("exec/nodes.i"), NODES).unwrap();
        std::fs::write(root.join("exec/ports.i"), PORTS).unwrap();
        let names = read(&root);
        let _ = std::fs::remove_dir_all(&root);
        let value = |n: &str| names.get(n).map(|n| n.value);
        assert_eq!(value("LN_NAME"), Some(10));
        assert_eq!(value("LN_SIZE"), Some(14));
        assert_eq!(value("NT_TASK"), Some(1));
        assert_eq!(value("MP_FLAGS"), Some(14), "a structure built on a Node starts where it ends");
        assert_eq!(value("MP_MSGLIST"), Some(20));
        assert_eq!(value("MP_LIST"), Some(20));
        assert_eq!(value("MP_SIZE"), Some(36), "aligned to a long word after the 14 bytes of the list");
        assert_eq!(value("MEMB_CHIP"), Some(1));
        assert_eq!(value("MEMF_CHIP"), Some(2));
        assert_eq!(value("MODE_OLDFILE"), Some(1005), "what follows a space is a comment");
        assert_eq!(value("SECOND"), Some(5));
        assert!(value("SOFFSET").is_none() && value("EXEC_TYPES_I").is_none(), "what SET gives isn't kept");
        assert_eq!(names["LN_NAME"].describe("LN_NAME"), "LN_NAME: offset 10 in LN (APTR, 4 bytes)\nexec/nodes.i");
        assert_eq!(names["LN_SIZE"].describe("LN_SIZE"), "LN_SIZE: the size of LN, 14 bytes\nexec/nodes.i");
        assert_eq!(names["MP_MSGLIST"].describe("MP_MSGLIST"), "MP_MSGLIST: offset 20 in MP\nexec/ports.i");
        assert_eq!(names["MP_LIST"].describe("MP_LIST"), "MP_LIST: offset 20 in MP (STRUCT, 14 bytes)\nexec/ports.i");
        assert_eq!(names["MEMF_CHIP"].describe("MEMF_CHIP"), "MEMF_CHIP = 2 ($2): bit 1 (MEMB_CHIP)\nexec/ports.i");
    }
}
