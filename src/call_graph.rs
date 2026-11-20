//! An assembly program's call graph: its routines, and which calls which, found in what Limoniter's own
//! assembler made of it. A call is an instruction that calls (the Z80's CALL and RST, the 6502's JSR, the 6809's
//! JSR, BSR and LBSR, the 68000's JSR and BSR, the x86's CALL and the ARM's BL), read from the bytes the line
//! became, whose target is a label the line names. A routine is a label that code starts at; each call belongs to
//! the routine above it.

use crate::asm::Assembly;
use crate::assemblers::Cpu;
use crate::diagram::{self, Align, Direction, Edge, Gaps, Item, Node, Paint, Scene};
use std::collections::{BTreeMap, HashMap};

/// A routine: its label, where it's defined, and its address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Routine {
    pub name: String,
    pub file: usize,
    pub line: usize,
    pub address: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CallGraph {
    pub routines: Vec<Routine>,
    /// Who calls whom (by their places in `routines`), and from how many places.
    pub calls: Vec<(usize, usize, usize)>,
    /// The routine the program starts at, if it's one of them.
    pub entry: Option<usize>,
    /// Made from a language server's call hierarchy, so each routine has a file and line but no address.
    pub from_server: bool,
}

/// A language server's call hierarchy, gathered a request at a time: its items (functions, methods) and calls.
#[derive(Default)]
pub struct ServerGraph {
    graph: CallGraph,
    pub files: Vec<std::path::PathBuf>,
    keys: Vec<(String, i64)>,
}

impl ServerGraph {
    /// A call hierarchy item, as the server describes it: added if it's new. Its place in the graph.
    pub fn item(&mut self, item: &serde_json::Value) -> Option<usize> {
        let uri = item.get("uri")?.as_str()?;
        let name = item.get("name")?.as_str()?.to_string();
        let line = item.pointer("/selectionRange/start/line").or_else(|| item.pointer("/range/start/line")).and_then(serde_json::Value::as_i64).unwrap_or(0);
        let key = (uri.to_string(), line);
        if let Some(at) = self.keys.iter().position(|k| *k == key) {
            return Some(at);
        }
        let path = crate::lsp::path_of(uri)?;
        let file = match self.files.iter().position(|f| *f == path) {
            Some(at) => at,
            None => {
                self.files.push(path);
                self.files.len() - 1
            }
        };
        self.keys.push(key);
        self.graph.routines.push(Routine { name, file, line: line.max(0) as usize, address: 0 });
        Some(self.graph.routines.len() - 1)
    }

    /// A call from one item to another, from as many places as the server lists.
    pub fn call(&mut self, from: usize, to: usize, places: usize) {
        match self.graph.calls.iter_mut().find(|(a, b, _)| *a == from && *b == to) {
            Some(call) => call.2 = call.2.max(places.max(1)),
            None => self.graph.calls.push((from, to, places.max(1))),
        }
    }

    pub fn len(&self) -> usize {
        self.graph.routines.len()
    }

    /// The graph, starting from the first item.
    pub fn finish(mut self) -> (CallGraph, Vec<std::path::PathBuf>) {
        self.graph.entry = (!self.graph.routines.is_empty()).then_some(0);
        self.graph.from_server = true;
        (self.graph, self.files)
    }
}

/// Where a call instruction at an address goes, read from its first bytes; None if it isn't one, or goes
/// somewhere only known when it runs.
pub fn call_target(cpu: Cpu, bytes: &[u8], address: i64) -> Option<i64> {
    let byte = |n: usize| bytes.get(n).copied().map(i64::from);
    let word_le = |n: usize| Some(byte(n)? | byte(n + 1)? << 8);
    let word_be = |n: usize| Some(byte(n)? << 8 | byte(n + 1)?);
    match cpu {
        Cpu::Z80 => match bytes.first()? {
            0xCD | 0xC4 | 0xCC | 0xD4 | 0xDC | 0xE4 | 0xEC | 0xF4 | 0xFC => word_le(1),
            op @ (0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF) => Some(i64::from(op & 0x38)),
            _ => None,
        },
        Cpu::M6502 => (*bytes.first()? == 0x20).then(|| word_le(1)).flatten(),
        // The Game Boy's CALLs (but not those on the half-carry and parity flags it hasn't got) and RSTs.
        Cpu::Sm83 => match bytes.first()? {
            0xCD | 0xC4 | 0xCC | 0xD4 | 0xDC => word_le(1),
            op @ (0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF) => Some(i64::from(op & 0x38)),
            _ => None,
        },
        // The SPC700's CALL !abs, and PCALL into the top page; TCALL goes through a table only known when it runs.
        Cpu::Spc700 => match bytes.first()? {
            0x3F => word_le(1),
            0x4F => Some(0xFF00 | byte(1)?),
            _ => None,
        },
        Cpu::M6809 => match bytes.first()? {
            0xBD => word_be(1),
            0x8D => Some(address + 2 + i64::from(*bytes.get(1)? as i8)),
            0x17 => Some(address + 3 + i64::from(word_be(1)? as u16 as i16)),
            _ => None,
        },
        Cpu::M68k => {
            let op = word_be(0)?;
            if op & 0xFF00 == 0x6100 {
                let short = op & 0xFF;
                return match short {
                    0 => Some(address + 2 + i64::from(word_be(2)? as u16 as i16)),
                    0xFF => None,
                    _ => Some(address + 2 + i64::from(short as u8 as i8)),
                };
            }
            match op {
                0x4EB9 => Some(word_be(2)? << 16 | word_be(4)?),
                0x4EB8 => Some(i64::from(word_be(2)? as u16 as i16)),
                0x4EBA => Some(address + 2 + i64::from(word_be(2)? as u16 as i16)),
                _ => None,
            }
        }
        Cpu::X86 => {
            if *bytes.first()? != 0xE8 {
                return None;
            }
            match bytes.len() {
                3 => Some((address + 3 + i64::from(word_le(1)? as u16 as i16)) & 0xFFFF),
                5 => Some(address + 5 + i64::from((word_le(1)? | word_le(3)? << 16) as u32 as i32)),
                _ => None,
            }
        }
        Cpu::Arm => {
            let word = u32::from_le_bytes([*bytes.first()?, *bytes.get(1)?, *bytes.get(2)?, *bytes.get(3)?]);
            // B with the link bit: a BL, under any condition but the never one.
            if word >> 28 == 0xF || (word >> 24) & 0xF != 0xB {
                return None;
            }
            let offset = ((word & 0x00FF_FFFF) << 8) as i32 >> 6;
            Some(address + 8 + i64::from(offset))
        }
    }
}

/// The call graph of an assembled program.
pub fn build(assembly: &Assembly, cpu: Cpu) -> CallGraph {
    // Each line's code, by file and line.
    let mut placed: HashMap<(usize, usize), (i64, &[u8])> = HashMap::new();
    for p in assembly.lines.iter().filter(|p| !p.bytes.is_empty()) {
        placed.entry((p.file, p.line)).or_insert((p.address, &p.bytes));
    }
    // The addresses code starts at, in each file by line, so a label can be checked against the code after it.
    let mut code_after: HashMap<usize, BTreeMap<usize, i64>> = HashMap::new();
    for ((file, line), (address, _)) in &placed {
        code_after.entry(*file).or_default().insert(*line, *address);
    }
    let starts_code = |file: usize, line: usize, value: i64| code_after.get(&file).and_then(|lines| lines.range(line..).next()).is_some_and(|(_, address)| *address == value);
    // The calls: each line whose code is a call to a label that line names.
    let mut call_lines: Vec<(usize, usize, String)> = Vec::new();
    for used in &assembly.uses {
        let Some((address, bytes)) = placed.get(&(used.file, used.line)) else { continue };
        let Some(&value) = assembly.symbols.get(&used.name) else { continue };
        if call_target(cpu, bytes, *address) == Some(value) && !call_lines.iter().any(|(f, l, _)| (*f, *l) == (used.file, used.line)) {
            call_lines.push((used.file, used.line, used.name.clone()));
        }
    }
    // Routines: the labels that start code, local ones too if something calls them.
    let called: std::collections::HashSet<&str> = call_lines.iter().map(|(_, _, name)| name.as_str()).collect();
    let mut routines: Vec<Routine> = Vec::new();
    for d in &assembly.definitions {
        let Some(&value) = assembly.symbols.get(&d.name) else { continue };
        let local = d.written != d.name;
        if (local && !called.contains(d.name.as_str())) || !starts_code(d.file, d.line, value) || routines.iter().any(|r| r.name == d.name) {
            continue;
        }
        routines.push(Routine { name: d.name.clone(), file: d.file, line: d.line, address: value });
    }
    let index: HashMap<&str, usize> = routines.iter().enumerate().map(|(n, r)| (r.name.as_str(), n)).collect();
    let mut counted: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for (file, line, name) in &call_lines {
        // The routine the call is in: the last one defined above it in the same file.
        let caller = routines.iter().enumerate().filter(|(_, r)| r.file == *file && r.line <= *line).max_by_key(|(_, r)| r.line).map(|(n, _)| n);
        if let (Some(caller), Some(&callee)) = (caller, index.get(name.as_str())) {
            *counted.entry((caller, callee)).or_insert(0) += 1;
        }
    }
    // Only the routines in a call, and where it starts, are kept.
    let entry_address = assembly.entry;
    let mut keep: Vec<bool> = routines.iter().map(|r| Some(r.address) == entry_address).collect();
    for (caller, callee) in counted.keys() {
        keep[*caller] = true;
        keep[*callee] = true;
    }
    let mut renumber = vec![usize::MAX; routines.len()];
    let mut kept = Vec::new();
    for (n, routine) in routines.into_iter().enumerate() {
        if keep[n] {
            renumber[n] = kept.len();
            kept.push(routine);
        }
    }
    let calls = counted.into_iter().map(|((a, b), count)| (renumber[a], renumber[b], count)).collect();
    let entry = kept.iter().position(|r| Some(r.address) == entry_address);
    CallGraph { routines: kept, calls, entry, from_server: false }
}

/// The most routines drawn: a graph larger than this is too dense to read, and slow to lay out.
pub const MOST: usize = 300;

const SIZE: f32 = 12.0;

/// The graph drawn, and each routine's box in it, for clicking. Under each name goes its address, or for a language
/// server's graph its file and line.
pub fn draw(graph: &CallGraph, files: &[std::path::PathBuf], direction: Direction, here: Option<usize>) -> (Scene, Vec<egui_rect::Rect>) {
    let count = graph.routines.len().min(MOST);
    let place = |r: &Routine| match graph.from_server {
        true => format!("{}:{}", files.get(r.file).and_then(|f| f.file_name()).map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(), r.line + 1),
        false => format!("${:04X}", r.address & 0xFFFF_FFFF),
    };
    let labels: Vec<(String, String)> = graph.routines[..count].iter().map(|r| (r.name.clone(), place(r))).collect();
    let sizes: Vec<(f32, f32)> = labels.iter().map(|(name, address)| ((diagram::text_width(name, SIZE, true).max(diagram::text_width(address, 10.0, false)) + 20.0).max(60.0), SIZE * 2.6 + 6.0)).collect();
    let nodes: Vec<Node> = sizes.iter().map(|(w, h)| Node { width: *w, height: *h, group: None }).collect();
    let calls: Vec<&(usize, usize, usize)> = graph.calls.iter().filter(|(a, b, _)| *a < count && *b < count).collect();
    let edges: Vec<Edge> = calls.iter().map(|(a, b, n)| Edge { from: *a, to: *b, label: (*n > 1).then(|| (diagram::text_width(&format!("\u{D7}{n}"), 10.0, false) + 6.0, 13.0)) }).collect();
    let layout = diagram::layered(&nodes, &edges, direction, &Gaps { node: 24.0, rank: 44.0 });
    let mut scene = Scene::default();
    let shapes = vec![diagram::Shape::Round; count];
    for (n, (from, to, times)) in calls.iter().enumerate() {
        let route = &layout.routes[n];
        if route.len() < 2 {
            continue;
        }
        let mut points = route.clone();
        if from != to {
            let last = points.len() - 1;
            points[0] = diagram::edge_point(shapes[*from], route[0].0, route[0].1, sizes[*from].0, sizes[*from].1, route[1]);
            points[last] = diagram::edge_point(shapes[*to], route[last].0, route[last].1, sizes[*to].0, sizes[*to].1, route[last - 1]);
        }
        let points = diagram::smooth(&points);
        let k = points.len();
        let paint = if here.is_some_and(|h| h == *from || h == *to) { Paint::Accent } else { Paint::Line };
        scene.line(points.clone(), paint, 1.1, false);
        diagram::head(&mut scene, diagram::Head::Arrow, points[k - 1], points[k - 2], paint);
        if let Some((x, y)) = layout.labels[n] {
            scene.text(x, y, format!("\u{D7}{times}"), 10.0, Align::Middle, false, Paint::Faint);
        }
    }
    let mut boxes = Vec::with_capacity(count);
    for (n, (name, address)) in labels.iter().enumerate() {
        let (cx, cy) = layout.centres[n];
        let (w, h) = sizes[n];
        let stroke = if Some(n) == here || Some(n) == graph.entry { Paint::Accent } else { Paint::Line };
        scene.items.push(Item::Rect { x: cx - w / 2.0, y: cy - h / 2.0, w, h, radius: 5.0, fill: Paint::Fill, stroke, dashed: false });
        if Some(n) == here {
            scene.items.push(Item::Rect { x: cx - w / 2.0 - 2.0, y: cy - h / 2.0 - 2.0, w: w + 4.0, h: h + 4.0, radius: 6.0, fill: Paint::None, stroke: Paint::Accent, dashed: false });
        }
        scene.text(cx, cy - SIZE * 0.55, name.clone(), SIZE, Align::Middle, true, Paint::Text);
        scene.text(cx, cy + SIZE * 0.65, address.clone(), 10.0, Align::Middle, false, Paint::Faint);
        boxes.push(egui_rect::Rect { x: cx - w / 2.0, y: cy - h / 2.0, w, h });
    }
    // Moved together with the scene, so the boxes stay where they're drawn.
    let before = scene.items.iter().find_map(|i| if let Item::Rect { x, y, .. } = i { Some((*x, *y)) } else { None });
    scene.fit(10.0);
    let after = scene.items.iter().find_map(|i| if let Item::Rect { x, y, .. } = i { Some((*x, *y)) } else { None });
    if let (Some(b), Some(a)) = (before, after) {
        for r in &mut boxes {
            r.x += a.0 - b.0;
            r.y += a.1 - b.1;
        }
    }
    (scene, boxes)
}

/// A box in a scene's points, without needing egui to describe it.
pub mod egui_rect {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Rect {
        pub x: f32,
        pub y: f32,
        pub w: f32,
        pub h: f32,
    }

    impl Rect {
        pub fn contains(&self, x: f32, y: f32) -> bool {
            x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
        }
    }
}

/// The routine a source line is in.
pub fn routine_at(graph: &CallGraph, file: usize, line: usize) -> Option<usize> {
    graph.routines.iter().enumerate().filter(|(_, r)| r.file == file && r.line <= line).max_by_key(|(_, r)| r.line).map(|(n, _)| n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_game_boys_and_the_spc700s_calls_are_found() {
        assert_eq!(call_target(Cpu::Sm83, &[0xCD, 0x34, 0x12], 0x150), Some(0x1234));
        assert_eq!(call_target(Cpu::Sm83, &[0xDF], 0x150), Some(0x18));
        assert_eq!(call_target(Cpu::Sm83, &[0xC3, 0x34, 0x12], 0x150), None, "JP isn't a call");
        assert_eq!(call_target(Cpu::Spc700, &[0x3F, 0x00, 0x08], 0x200), Some(0x0800));
        assert_eq!(call_target(Cpu::Spc700, &[0x4F, 0x20], 0x200), Some(0xFF20));
        assert_eq!(call_target(Cpu::Spc700, &[0x01], 0x200), None);
    }

    #[test]
    fn a_language_servers_hierarchy_becomes_a_graph() {
        let item = |name: &str, line: i64| serde_json::json!({ "name": name, "kind": 12, "uri": "file:///work/main.rs", "range": { "start": { "line": line, "character": 0 }, "end": { "line": line + 3, "character": 1 } }, "selectionRange": { "start": { "line": line, "character": 3 }, "end": { "line": line, "character": 7 } } });
        let mut gathered = ServerGraph::default();
        let main = gathered.item(&item("main", 0)).unwrap();
        let run = gathered.item(&item("run", 10)).unwrap();
        let again = gathered.item(&item("main", 0)).unwrap();
        assert_eq!(main, again, "the same item isn't added twice");
        gathered.call(main, run, 2);
        gathered.call(main, run, 1);
        assert!(gathered.item(&serde_json::json!({ "name": "no place" })).is_none());
        let (graph, files) = gathered.finish();
        assert_eq!(graph.calls, [(0, 1, 2)]);
        assert_eq!(graph.routines[1].line, 10);
        assert_eq!(files.len(), 1);
        let (scene, _) = draw(&graph, &files, Direction::Down, None);
        assert!(scene.words().contains(&"main.rs:11"), "{:?}", scene.words());
    }

    fn assemble(cpu: Cpu, source: &str) -> Assembly {
        let read = |_: &Path| -> Result<Vec<u8>, String> { Err(String::new()) };
        let assembly = crate::asm::assemble(cpu.isa(), Path::new("test.asm"), source, &read);
        assert!(assembly.problems.is_empty(), "{:?}", assembly.problems);
        assembly
    }

    fn calls(graph: &CallGraph) -> Vec<(String, String, usize)> {
        graph.calls.iter().map(|(a, b, n)| (graph.routines[*a].name.clone(), graph.routines[*b].name.clone(), *n)).collect()
    }

    #[test]
    fn a_z80_programs_calls_are_found_and_data_isnt_taken_for_them() {
        let source = "  org $8000\nstart:\n  call init\n  call nz,draw\n  call draw\n  ret\ninit:\n  ld a,1\n  ret\ndraw:\n  call init\n  ret\ntable:\n  db $CD\n  dw init\nunused:\n  ret\n";
        let graph = build(&assemble(Cpu::Z80, source), Cpu::Z80);
        assert_eq!(calls(&graph), [("start".into(), "init".into(), 1), ("start".into(), "draw".into(), 2), ("draw".into(), "init".into(), 1)]);
        let names: Vec<&str> = graph.routines.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["start", "init", "draw"], "a routine in no call is left out, and data isn't code");
        assert_eq!(graph.routines[1].line, 6);
        assert_eq!(routine_at(&graph, 0, 8), Some(1));
    }

    #[test]
    fn each_processors_calls_are_read_from_their_bytes() {
        let six502 = build(&assemble(Cpu::M6502, "  * = $c000\nmain jsr sub\n  rts\nsub lda #1\n  rts\n"), Cpu::M6502);
        assert_eq!(calls(&six502), [("main".into(), "sub".into(), 1)]);
        let m6809 = build(&assemble(Cpu::M6809, "  org $4000\nmain bsr one\n  lbsr two\n  jsr three\n  rts\none rts\ntwo rts\nthree rts\n"), Cpu::M6809);
        assert_eq!(calls(&m6809).len(), 3, "{:?}", calls(&m6809));
        let m68k = build(&assemble(Cpu::M68k, "  org $1000\nmain:\n  bsr.s one\n  bsr.w two\n  jsr three\n  rts\none:\n  rts\ntwo:\n  rts\nthree:\n  rts\n"), Cpu::M68k);
        assert_eq!(calls(&m68k).len(), 3, "{:?}", calls(&m68k));
        let x86 = build(&assemble(Cpu::X86, "  org 0x100\nmain:\n  call sub\n  ret\nsub:\n  ret\n"), Cpu::X86);
        assert_eq!(calls(&x86), [("main".into(), "sub".into(), 1)]);
        let arm = build(&assemble(Cpu::Arm, "main\n  BL work\n  MOV pc,lr\nwork\n  MOV pc,lr\n"), Cpu::Arm);
        assert_eq!(calls(&arm), [("main".into(), "work".into(), 1)]);
        assert_eq!(call_target(Cpu::Z80, &[0xFF], 0), Some(0x38), "RST 38H");
    }

    #[test]
    fn the_graph_is_drawn_with_a_box_for_each_routine() {
        let source = "  org $8000\nstart:\n  call one\n  call two\n  ret\none:\n  call two\n  ret\ntwo:\n  ret\n";
        let assembly = assemble(Cpu::Z80, source);
        let graph = build(&assembly, Cpu::Z80);
        assert_eq!(graph.entry, None);
        let (scene, boxes) = draw(&graph, &[], Direction::Down, Some(1));
        assert_eq!(boxes.len(), 3);
        assert!(scene.words().contains(&"start") && scene.words().contains(&"$8000"));
        // Each box's place is where its name is drawn.
        for (n, rect) in boxes.iter().enumerate() {
            let found = scene.items.iter().any(|i| matches!(i, Item::Text { x, y, text, .. } if *text == graph.routines[n].name && rect.contains(*x, *y)));
            assert!(found, "{} is in its box", graph.routines[n].name);
        }
    }
}
