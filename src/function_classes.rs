//! The Functions panel by class: which class each function the grammar finds belongs to, whether it's public or
//! private, and its signature as it reads, worked out from the lines themselves. A class's members are the lines
//! indented past its own line until the code comes back to that indent, which is how classes are written in
//! languages with braces and in Python alike.

/// One function or class found in a file, as the grammar found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub line: usize,
    pub name: String,
    pub is_class: bool,
}

/// One entry of the panel's list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outlined {
    pub line: usize,
    /// A nested class is named with the classes it's inside: Outer.Inner.
    pub name: String,
    pub is_class: bool,
    /// The class it belongs to, by its place in the list; none for a function outside every class.
    pub class: Option<usize>,
    pub private: bool,
    /// The function's line or lines up to its body, for its hover: its parameters and what it returns.
    pub signature: String,
}

/// How the panel orders what it lists. Classes and the functions outside them are never mixed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Order {
    /// As the file has them.
    #[default]
    Natural,
    Alphabetical,
    /// Within each class (and among the functions outside them), the private ones A to Z, then the public ones.
    PrivateFirst,
}

impl Order {
    pub const ALL: [Order; 3] = [Order::Natural, Order::Alphabetical, Order::PrivateFirst];

    pub fn id(self) -> &'static str {
        match self {
            Order::Natural => "natural",
            Order::Alphabetical => "az",
            Order::PrivateFirst => "private",
        }
    }

    pub fn from_id(id: &str) -> Option<Order> {
        Order::ALL.into_iter().find(|order| order.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Order::Natural => "As in the file",
            Order::Alphabetical => "A to Z",
            Order::PrivateFirst => "Private A to Z, then public",
        }
    }
}

/// How far a line is indented, a tab counting as four.
fn indent(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

/// Whether the language's functions are private unless they say otherwise.
fn private_unless_said(language: &str) -> bool {
    matches!(language, "Rust" | "C#")
}

/// Whether a function is private: by the words before its name (pub, public, private, protected…), its name
/// (a leading _ or #, though not Python's __name__), the section of a C++ class it's in, or the language's way.
fn is_private(language: &str, line: &str, name: &str, section: Option<bool>) -> bool {
    let before = line.find(name).map_or(line, |at| &line[..at]);
    let words: Vec<&str> = before.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).collect();
    if words.iter().any(|w| matches!(*w, "private" | "protected" | "fileprivate")) {
        return true;
    }
    if words.iter().any(|w| matches!(*w, "pub" | "public" | "export" | "internal" | "open")) {
        return false;
    }
    let bare = name.rsplit(['.', ':']).next().unwrap_or(name);
    if bare.starts_with('#') || bare.starts_with('_') && !(bare.starts_with("__") && bare.ends_with("__")) {
        return true;
    }
    section.unwrap_or_else(|| private_unless_said(language))
}

/// The function's signature, from its line and those after it while its brackets are open, up to its body.
fn signature(lines: &[&str], at: usize) -> String {
    let mut depth = 0i32;
    let mut out = String::new();
    for line in lines.iter().skip(at).take(8) {
        let mut piece = String::new();
        let mut done = false;
        for c in line.trim().chars() {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                '{' if depth <= 0 => {
                    done = true;
                    break;
                }
                _ => {}
            }
            piece.push(c);
        }
        if !out.is_empty() && !piece.is_empty() {
            out.push(' ');
        }
        out.push_str(piece.trim());
        if done || depth <= 0 {
            break;
        }
    }
    let out = out.trim_end();
    out.strip_suffix(':').unwrap_or(out).trim_end().to_string()
}

/// A Go method's receiver type: `func (s *Server) Start()` belongs to Server.
fn go_receiver(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix("func")?.trim_start().strip_prefix('(')?;
    let inside = &rest[..rest.find(')')?];
    let kind = inside.split_whitespace().last()?.trim_start_matches('*');
    let kind = kind.split('[').next().unwrap_or(kind);
    (!kind.is_empty()).then(|| kind.to_string())
}

/// Works out each function's class, privacy and signature. `code` says which lines start in code rather than inside
/// a string or comment: only those can end a class.
pub fn arrange(language: &str, lines: &[&str], code: &[bool], found: &[Found]) -> Vec<Outlined> {
    let mut out: Vec<Outlined> = Vec::with_capacity(found.len());
    // The classes open at this line: their place in `out`, their indent, and the C++ section they're in.
    let mut open: Vec<(usize, usize, Option<bool>)> = Vec::new();
    let mut next = found.iter().peekable();
    let mut line_no = 0;
    while line_no < lines.len() && next.peek().is_some() {
        let line = lines[line_no];
        let here = indent(line);
        let text = line.trim();
        // A C++ class's sections, often written at the class's own indent.
        let section = match text {
            "private:" | "protected:" => Some(true),
            "public:" => Some(false),
            _ => None,
        };
        if let (Some(section), Some(last)) = (section, open.last_mut()) {
            last.2 = Some(section);
        } else if !text.is_empty() && code.get(line_no).copied().unwrap_or(true) && !text.starts_with('{') {
            while open.last().is_some_and(|&(place, at, _)| here <= at && out[place].line != line_no) {
                open.pop();
            }
        }
        while let Some(entry) = next.next_if(|entry| entry.line == line_no) {
            let class = open.last().map(|&(place, _, _)| place);
            let section = open.last().and_then(|&(_, _, section)| section);
            let name = match class.filter(|_| entry.is_class) {
                Some(outer) => format!("{}.{}", out[outer].name, entry.name),
                None => entry.name.clone(),
            };
            out.push(Outlined { line: entry.line, private: !entry.is_class && is_private(language, line, &entry.name, section), signature: if entry.is_class { line.trim().trim_end_matches('{').trim_end().to_string() } else { signature(lines, entry.line) }, name, is_class: entry.is_class, class });
            if entry.is_class {
                open.push((out.len() - 1, here, None));
            }
        }
        line_no += 1;
    }
    // Functions written outside their class, by the class's name: C++'s Shape::area and Go's receivers.
    for place in 0..out.len() {
        if out[place].is_class || out[place].class.is_some() {
            continue;
        }
        let owner = if language == "Go" { go_receiver(lines.get(out[place].line).copied().unwrap_or_default()) } else { out[place].name.rsplit_once("::").map(|(owner, _)| owner.to_string()) };
        let Some(owner) = owner else { continue };
        let class = match out.iter().position(|entry| entry.is_class && entry.name == owner) {
            Some(class) => class,
            None => {
                out.push(Outlined { line: out[place].line, name: owner.clone(), is_class: true, signature: owner, ..Outlined::default() });
                out.len() - 1
            }
        };
        out[place].class = Some(class);
    }
    out
}

/// What the panel shows, in order: each class's heading and, unless it's folded, its functions; then the functions
/// outside every class, under a heading of their own when there are classes. Rows are places in `items`, with None
/// for the heading over the functions outside.
pub fn rows(items: &[Outlined], order: Order, filter: &str, folded: &std::collections::HashSet<String>) -> Vec<Option<usize>> {
    let filter = filter.trim().to_lowercase();
    let matches = |entry: &Outlined| filter.is_empty() || entry.name.to_lowercase().contains(&filter);
    let sort = |places: &mut Vec<usize>| match order {
        Order::Natural => {}
        Order::Alphabetical => places.sort_by_cached_key(|&p| (items[p].name.to_lowercase(), items[p].line)),
        Order::PrivateFirst => places.sort_by_cached_key(|&p| (!items[p].private, items[p].name.to_lowercase(), items[p].line)),
    };
    let mut classes: Vec<usize> = (0..items.len()).filter(|&p| items[p].is_class).collect();
    if order != Order::Natural {
        classes.sort_by_cached_key(|&p| (items[p].name.to_lowercase(), items[p].line));
    }
    let mut out = Vec::new();
    for class in &classes {
        let mut members: Vec<usize> = (0..items.len()).filter(|&p| !items[p].is_class && items[p].class == Some(*class) && matches(&items[p])).collect();
        if members.is_empty() && !matches(&items[*class]) {
            continue;
        }
        sort(&mut members);
        out.push(Some(*class));
        if !folded.contains(&items[*class].name) {
            out.extend(members.into_iter().map(Some));
        }
    }
    let mut outside: Vec<usize> = (0..items.len()).filter(|&p| !items[p].is_class && items[p].class.is_none() && matches(&items[p])).collect();
    sort(&mut outside);
    if !classes.is_empty() && !outside.is_empty() {
        out.push(None);
    }
    out.extend(outside.into_iter().map(Some));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(list: &[(usize, &str, bool)]) -> Vec<Found> {
        list.iter().map(|&(line, name, is_class)| Found { line, name: name.to_string(), is_class }).collect()
    }

    fn arranged(language: &str, text: &[&str], list: &[(usize, &str, bool)]) -> Vec<Outlined> {
        arrange(language, text, &vec![true; text.len()], &found(list))
    }

    #[test]
    fn functions_belong_to_the_class_they_are_indented_in() {
        let text = ["class Shape:", "    def area(self) -> float:", "        return 0", "    def _scale(self, by):", "        pass", "", "def helper(x):", "    return x", "class Empty:", "    pass"];
        let out = arranged("Python", &text, &[(0, "Shape", true), (1, "area", false), (3, "_scale", false), (6, "helper", false), (8, "Empty", true)]);
        assert_eq!(out.iter().map(|e| e.class).collect::<Vec<_>>(), [None, Some(0), Some(0), None, None]);
        assert_eq!(out[1].signature, "def area(self) -> float");
        assert!(!out[1].private && out[2].private && !out[3].private);
    }

    #[test]
    fn braces_rust_and_cpp_sections_and_owners_written_outside() {
        let rust = ["impl Point {", "    pub fn new(x: i32,", "               y: i32) -> Self {", "    }", "    fn len(&self) -> f32 { 0.0 }", "}", "fn main() {}"];
        let out = arranged("Rust", &rust, &[(0, "Point", true), (1, "new", false), (4, "len", false), (6, "main", false)]);
        assert_eq!(out.iter().map(|e| e.class).collect::<Vec<_>>(), [None, Some(0), Some(0), None]);
        assert_eq!(out[1].signature, "pub fn new(x: i32, y: i32) -> Self");
        assert!(!out[1].private && out[2].private, "Rust is private unless it says pub");
        let cpp = ["class A", "{", "public:", "    void shown();", "private:", "    void kept();", "};", "void A::shown() {}", "int B::size() { return 0; }"];
        let out = arranged("C++", &cpp, &[(0, "A", true), (3, "shown", false), (5, "kept", false), (7, "A::shown", false), (8, "B::size", false)]);
        assert_eq!(out.iter().map(|e| e.class).collect::<Vec<_>>(), [None, Some(0), Some(0), Some(0), Some(5), None]);
        assert!(!out[1].private && out[2].private);
        assert_eq!(out[5].name, "B");
        let go = ["func (s *Server) Start() error {", "}"];
        assert_eq!(arranged("Go", &go, &[(0, "Start", false)])[1].name, "Server");
    }

    #[test]
    fn a_line_inside_a_string_does_not_end_a_class() {
        let text = ["class A:", "    def one(self):", "        s = '''", "text at the left", "'''", "    def two(self):", "        pass"];
        let code = [true, true, true, false, false, true, true];
        let out = arrange("Python", &text, &code, &found(&[(0, "A", true), (1, "one", false), (5, "two", false)]));
        assert_eq!(out[2].class, Some(0));
    }

    #[test]
    fn rows_keep_classes_and_loose_functions_apart_in_each_order() {
        let text = ["class Zed:", "    def b(self): pass", "    def _a(self): pass", "class Alpha:", "    def c(self): pass", "def loose(): pass", "def _also(): pass"];
        let items = arranged("Python", &text, &[(0, "Zed", true), (1, "b", false), (2, "_a", false), (3, "Alpha", true), (4, "c", false), (5, "loose", false), (6, "_also", false)]);
        let none = std::collections::HashSet::new();
        let names = |rows: Vec<Option<usize>>| rows.into_iter().map(|row| row.map_or("--".to_string(), |p| items[p].name.clone())).collect::<Vec<_>>();
        assert_eq!(names(rows(&items, Order::Natural, "", &none)), ["Zed", "b", "_a", "Alpha", "c", "--", "loose", "_also"]);
        assert_eq!(names(rows(&items, Order::Alphabetical, "", &none)), ["Alpha", "c", "Zed", "_a", "b", "--", "_also", "loose"]);
        assert_eq!(names(rows(&items, Order::PrivateFirst, "", &none)), ["Alpha", "c", "Zed", "_a", "b", "--", "_also", "loose"]);
        let folded = ["Zed".to_string()].into();
        assert_eq!(names(rows(&items, Order::Natural, "", &folded)), ["Zed", "Alpha", "c", "--", "loose", "_also"]);
        assert_eq!(names(rows(&items, Order::Natural, "c", &none)), ["Alpha", "c"]);
        let flat = arranged("Python", &["def f(): pass"], &[(0, "f", false)]);
        assert_eq!(rows(&flat, Order::Natural, "", &none), [Some(0)], "no heading without classes");
        assert_eq!(Order::from_id(Order::PrivateFirst.id()), Some(Order::PrivateFirst));
    }
}
