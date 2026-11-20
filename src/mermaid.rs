//! Mermaid diagrams, drawn without Mermaid. Mermaid itself is JavaScript, and Limoniter runs none, so the
//! commonest kinds are read and laid out here: flowcharts, sequence diagrams, state and class diagrams,
//! entity-relationship diagrams, pie charts and Gantt charts. Anything else is shown as its text.

use crate::diagram::{self, Align, Direction, Edge, Gaps, Head, Item, Node, Paint, Scene, Shape};
use std::collections::HashMap;

/// The size words are drawn at.
const SIZE: f32 = 13.0;
const SMALL: f32 = 11.5;

/// Draws a diagram from its Mermaid text, or says why it can't.
pub fn render(source: &str) -> Result<Scene, String> {
    let lines = statements(source);
    let Some(first) = lines.first() else {
        return Err(crate::i18n::tr("This Mermaid diagram is empty.").to_string());
    };
    let kind = first.split_whitespace().next().unwrap_or("").to_string();
    let rest = &lines[1..];
    let mut scene = match kind.as_str() {
        "graph" | "flowchart" | "flowchart-elk" => flowchart(first, rest),
        "sequenceDiagram" => sequence(rest),
        "stateDiagram" | "stateDiagram-v2" => state(rest),
        "classDiagram" | "classDiagram-v2" => class(rest),
        "erDiagram" => entities(rest),
        "pie" => pie(first, rest),
        "gantt" => gantt(rest)?,
        _ => return Err(crate::i18n::tf!("Mermaid's \u{201C}{kind}\u{201D} diagrams aren't drawn here yet, so here is its text.")),
    };
    if scene.items.is_empty() {
        return Err(crate::i18n::tr("This Mermaid diagram has nothing in it to draw.").to_string());
    }
    scene.fit(8.0);
    Ok(scene)
}

/// A diagram drawn from its text, kept so it isn't laid out again on every frame.
pub fn cached(source: &str) -> std::sync::Arc<Result<Scene, String>> {
    static DRAWN: std::sync::Mutex<Option<HashMap<String, std::sync::Arc<Result<Scene, String>>>>> = std::sync::Mutex::new(None);
    let mut drawn = DRAWN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let drawn = drawn.get_or_insert_with(HashMap::new);
    if let Some(found) = drawn.get(source) {
        return found.clone();
    }
    // Typing changes the text each time, so old ones are let go of now and then.
    if drawn.len() > 48 {
        drawn.clear();
    }
    let made = std::sync::Arc::new(render(source));
    drawn.insert(source.to_string(), made.clone());
    made
}

/// The diagram's lines, without comments, blank lines or its front matter (the settings between --- lines).
/// Semicolons end statements too, except in a sequence diagram's message text.
fn statements(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut lines = source.lines().peekable();
    if lines.peek().is_some_and(|l| l.trim() == "---") {
        lines.next();
        for line in lines.by_ref() {
            if line.trim() == "---" {
                break;
            }
        }
    }
    let mut sequence = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            continue;
        }
        // An init directive, %%{init: ...}%%, is a comment here too.
        let trimmed = trimmed.split("%%").next().unwrap_or("").trim();
        if out.is_empty() {
            sequence = trimmed.starts_with("sequenceDiagram");
        }
        if sequence || trimmed.contains('"') {
            out.push(trimmed.trim_end_matches(';').to_string());
        } else {
            out.extend(trimmed.split(';').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string));
        }
    }
    out
}

/// Words as they're written in a diagram: quotes taken off, <br> as a new line, Markdown's ` and ** dropped.
fn words(text: &str) -> Vec<String> {
    let text = text.trim();
    let text = text.strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or(text);
    let text = text.strip_prefix('`').and_then(|t| t.strip_suffix('`')).unwrap_or(text);
    // Font Awesome's icons (fa:fa-car) can't be drawn here, so they're left out.
    let text = text.split(' ').filter(|w| !w.starts_with("fa:fa-") && !w.starts_with("fab:fa-")).collect::<Vec<_>>().join(" ");
    let text = text.replace("<br/>", "\n").replace("<br />", "\n").replace("<br>", "\n").replace("\\n", "\n").replace("**", "").replace("#quot;", "\"").replace("#amp;", "&").replace("#lt;", "<").replace("#gt;", ">");
    text.lines().map(|l| l.trim().to_string()).collect()
}

fn widest(lines: &[String], size: f32, bold: bool) -> f32 {
    lines.iter().map(|l| diagram::text_width(l, size, bold)).fold(0.0, f32::max)
}

fn height_of(lines: &[String], size: f32) -> f32 {
    lines.len().max(1) as f32 * size * diagram::LINE
}

// ---------- Flowcharts ----------

#[derive(Clone, Debug)]
struct FlowNode {
    text: Vec<String>,
    shape: Shape,
    group: Option<usize>,
}

#[derive(Clone, Debug)]
struct FlowEdge {
    from: usize,
    to: usize,
    text: Vec<String>,
    start: Head,
    end: Head,
    dashed: bool,
    thick: bool,
    hidden: bool,
}

#[derive(Clone, Debug)]
struct Group {
    title: Vec<String>,
    parent: Option<usize>,
}

#[derive(Default)]
struct Flow {
    nodes: Vec<FlowNode>,
    edges: Vec<FlowEdge>,
    groups: Vec<Group>,
    index: HashMap<String, usize>,
}

impl Flow {
    /// The node with an id, made if it's new, in the group being read; its words and shape if they're given.
    fn node(&mut self, id: &str, text: Option<Vec<String>>, shape: Option<Shape>, group: Option<usize>) -> usize {
        let at = match self.index.get(id) {
            Some(at) => *at,
            None => {
                self.nodes.push(FlowNode { text: vec![id.to_string()], shape: Shape::Rect, group });
                self.index.insert(id.to_string(), self.nodes.len() - 1);
                self.nodes.len() - 1
            }
        };
        if let Some(text) = text {
            self.nodes[at].text = text;
        }
        if let Some(shape) = shape {
            self.nodes[at].shape = shape;
        }
        // A node mentioned first outside a subgraph and then inside one belongs to it, as in Mermaid.
        if group.is_some() && self.nodes[at].group.is_none() {
            self.nodes[at].group = group;
        }
        at
    }
}

/// The brackets round a node's words, each with the shape it gives; longer ones first so they're found first.
const BRACKETS: [(&str, &str, Shape); 15] = [
    ("(((", ")))", Shape::DoubleCircle),
    ("([", "])", Shape::Stadium),
    ("[[", "]]", Shape::Subroutine),
    ("[(", ")]", Shape::Cylinder),
    ("((", "))", Shape::Circle),
    ("{{", "}}", Shape::Hexagon),
    ("[/", "/]", Shape::Lean(true)),
    ("[\\", "\\]", Shape::Lean(false)),
    ("[/", "\\]", Shape::Trapezoid(true)),
    ("[\\", "/]", Shape::Trapezoid(false)),
    ("(", ")", Shape::Round),
    ("[", "]", Shape::Rect),
    ("{", "}", Shape::Diamond),
    (">", "]", Shape::Flag),
    ("@{", "}", Shape::Round),
];

fn id_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.'
}

/// A node as it's written: its id, then perhaps its words in brackets, then perhaps :::its class. Gives the id,
/// words and shape, and how much of the text it took.
fn read_node(text: &str) -> Option<(String, Option<(Vec<String>, Shape)>, usize)> {
    let start = text.len() - text.trim_start().len();
    let rest = &text[start..];
    let id_len: usize = rest.char_indices().find(|(_, c)| !id_char(*c)).map_or(rest.len(), |(at, _)| at);
    // A dot can't end an id, as -.-> starts with one.
    let id_len = rest[..id_len].trim_end_matches('.').len();
    if id_len == 0 {
        return None;
    }
    let id = rest[..id_len].to_string();
    let mut used = start + id_len;
    let after = &text[used..];
    let mut shaped = None;
    for (open, close, shape) in BRACKETS {
        let Some(inside) = after.strip_prefix(open) else { continue };
        // Quoted words can hold anything, brackets included.
        let (words_text, end) = if inside.trim_start().starts_with('"') {
            let lead = inside.len() - inside.trim_start().len();
            let Some(quote_end) = inside[lead + 1..].find('"') else { continue };
            let after_quote = lead + 1 + quote_end + 1;
            let Some(close_at) = inside[after_quote..].find(close) else { continue };
            (&inside[lead..after_quote], after_quote + close_at)
        } else {
            let Some(close_at) = inside.find(close) else { continue };
            (&inside[..close_at], close_at)
        };
        // [/text/] and [/text\] start alike: the one that closes first is the one meant.
        if open == "[/" || open == "[\\" {
            let other = if close.starts_with('/') { "\\]" } else { "/]" };
            if inside.find(other).is_some_and(|o| o < end) {
                continue;
            }
        }
        if open == "@{" {
            // Mermaid's newer way of naming a shape, @{ shape: ..., label: "..." }: the label is kept.
            let label = words_text.split(',').find_map(|part| part.trim().strip_prefix("label:")).map(words);
            shaped = Some((label.unwrap_or_else(|| vec![id.clone()]), shape));
        } else {
            shaped = Some((words(words_text), shape));
        }
        used += open.len() + end + close.len();
        break;
    }
    if let Some(class) = text[used..].strip_prefix(":::") {
        used += 3 + class.char_indices().find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '-')).map_or(class.len(), |(at, _)| at);
    }
    Some((id, shaped, used))
}

/// A link between nodes: how it's drawn, its words, and how much text it took.
fn read_link(text: &str) -> Option<(Head, Head, bool, bool, bool, Vec<String>, usize)> {
    let start = text.len() - text.trim_start().len();
    let chars: Vec<(usize, char)> = text[start..].char_indices().collect();
    let mut k = 0;
    let mut start_head = Head::None;
    if chars.first().is_some_and(|(_, c)| matches!(c, '<' | 'o' | 'x')) && chars.get(1).is_some_and(|(_, c)| matches!(c, '-' | '=' | '.')) {
        start_head = match chars[0].1 {
            '<' => Head::Arrow,
            'o' => Head::Circle,
            _ => Head::Cross,
        };
        k = 1;
    }
    let body_start = k;
    while k < chars.len() && matches!(chars[k].1, '-' | '=' | '.' | '~') {
        k += 1;
    }
    let body: String = chars[body_start..k].iter().map(|(_, c)| *c).collect();
    // A single - or . isn't a link.
    if body.len() < 2 {
        return None;
    }
    let head_of = |c: char| match c {
        '>' => Some(Head::Arrow),
        'o' => Some(Head::Circle),
        'x' => Some(Head::Cross),
        _ => None,
    };
    let next_is_gap = |at: usize| chars.get(at).is_none_or(|(_, c)| c.is_whitespace() || *c == '|');
    let mut end_head = Head::None;
    if let Some((_, c)) = chars.get(k)
        && let Some(head) = head_of(*c)
        && (*c == '>' || next_is_gap(k + 1))
    {
        end_head = head;
        k += 1;
    }
    let mut label = Vec::new();
    let byte = |k: usize| start + chars.get(k).map_or(text.len() - start, |(at, _)| *at);
    // Words in the middle of a link: A -- words --> B, A -. words .-> B, A == words ==> B.
    if end_head == Head::None && matches!(body.as_str(), "--" | "==" | "-.") && chars.get(k).is_some_and(|(_, c)| c.is_whitespace()) {
        let rest = &text[byte(k)..];
        let closers = ["-->", "---", "==>", "===", ".->", ".-", "--o", "--x", "==o", "==x"];
        if let Some((at, closer)) = closers.iter().filter_map(|c| rest.find(c).map(|at| (at, *c))).min_by_key(|(at, _)| *at) {
            label = words(&rest[..at]);
            let mut used = byte(k) + at + closer.len();
            let mut tail = closer.to_string();
            // A longer link: ---->.
            while text[used..].starts_with(['-', '=', '.']) {
                tail.push(text[used..].chars().next().unwrap_or('-'));
                used += 1;
            }
            let end = if tail.ends_with('>') {
                Head::Arrow
            } else if tail.ends_with('o') {
                Head::Circle
            } else if tail.ends_with('x') {
                Head::Cross
            } else if text[used..].starts_with('>') {
                used += 1;
                Head::Arrow
            } else {
                Head::None
            };
            let dashed = body.contains('.') || closer.contains('.');
            let thick = body.contains('=');
            return Some((start_head, end, dashed, thick, false, label, used));
        }
    }
    let mut used = byte(k);
    let rest = &text[used..];
    let trimmed = rest.trim_start();
    if let Some(inside) = trimmed.strip_prefix('|')
        && let Some(close) = inside.find('|')
    {
        label = words(&inside[..close]);
        used += rest.len() - trimmed.len() + 1 + close + 1;
    }
    let dashed = body.contains('.');
    let thick = body.contains('=');
    let hidden = body.contains('~');
    Some((start_head, end_head, dashed, thick, hidden, label, used))
}

/// Nodes joined by &: A & B.
fn read_nodes(flow: &mut Flow, text: &str, group: Option<usize>) -> Option<(Vec<usize>, usize)> {
    let mut found = Vec::new();
    let mut used = 0;
    loop {
        let (id, shaped, took) = read_node(&text[used..])?;
        if id == "end" && shaped.is_none() && found.is_empty() {
            return None;
        }
        let (words, shape) = shaped.map_or((None, None), |(w, s)| (Some(w), Some(s)));
        found.push(flow.node(&id, words, shape, group));
        used += took;
        let rest = &text[used..];
        let trimmed = rest.trim_start();
        if let Some(after) = trimmed.strip_prefix('&') {
            used += rest.len() - after.len();
            continue;
        }
        return Some((found, used));
    }
}

fn flowchart(first: &str, lines: &[String]) -> Scene {
    let direction = first.split_whitespace().nth(1).and_then(Direction::from_mermaid).unwrap_or(Direction::Down);
    let mut flow = Flow::default();
    let mut stack: Vec<usize> = Vec::new();
    for line in lines {
        let word = line.split_whitespace().next().unwrap_or("");
        match word {
            "subgraph" => {
                let rest = line["subgraph".len()..].trim();
                // subgraph id[Title], subgraph id ["Title"], or subgraph Title with spaces.
                let title = match read_node(rest) {
                    Some((_, Some((words, _)), _)) => words,
                    Some((id, None, took)) => {
                        let after = rest[took..].trim();
                        match after.strip_prefix('[').and_then(|a| a.strip_suffix(']')) {
                            _ if after.is_empty() => vec![id],
                            Some(inner) => words(inner),
                            None => words(rest),
                        }
                    }
                    None => words(rest),
                };
                flow.groups.push(Group { title, parent: stack.last().copied() });
                stack.push(flow.groups.len() - 1);
                continue;
            }
            "end" => {
                stack.pop();
                continue;
            }
            "classDef" | "class" | "style" | "linkStyle" | "click" | "direction" | "accTitle" | "accDescr" | "accTitle:" | "accDescr:" => continue,
            _ => {}
        }
        let group = stack.last().copied();
        let Some((mut sources, mut used)) = read_nodes(&mut flow, line, group) else { continue };
        while let Some((start, end, dashed, thick, hidden, text, took)) = read_link(&line[used..]) {
            used += took;
            let Some((targets, took)) = read_nodes(&mut flow, &line[used..], group) else { break };
            used += took;
            for from in &sources {
                for to in &targets {
                    flow.edges.push(FlowEdge { from: *from, to: *to, text: text.clone(), start, end, dashed, thick, hidden });
                }
            }
            sources = targets;
        }
    }
    draw_flow(&flow, direction)
}

/// The size of a node's shape round its words.
fn shape_size(shape: Shape, text: &[String]) -> (f32, f32) {
    let (w, h) = (widest(text, SIZE, false), height_of(text, SIZE));
    match shape {
        Shape::Circle | Shape::DoubleCircle => {
            let d = (w + 20.0).max(h + 20.0).max(36.0);
            (d, d)
        }
        Shape::Diamond => ((w + 24.0) * 1.45 + 10.0, (h + 14.0) * 1.6),
        Shape::Hexagon => (w + 24.0 + h + 18.0, h + 18.0),
        Shape::Lean(_) | Shape::Trapezoid(_) => (w + 24.0 + (h + 18.0) * 0.8, h + 18.0),
        Shape::Stadium => (w + 24.0 + (h + 18.0) * 0.5, h + 18.0),
        Shape::Subroutine => (w + 34.0, h + 18.0),
        Shape::Cylinder => (w + 24.0, h + 30.0),
        Shape::Flag => (w + 34.0, h + 18.0),
        Shape::Start => (14.0, 14.0),
        Shape::End => (16.0, 16.0),
        Shape::Bar => (60.0, 7.0),
        Shape::Rect | Shape::Round => ((w + 24.0).max(40.0), h + 18.0),
    }
}

fn draw_flow(flow: &Flow, direction: Direction) -> Scene {
    let sizes: Vec<(f32, f32)> = flow.nodes.iter().map(|n| shape_size(n.shape, &n.text)).collect();
    let nodes: Vec<Node> = flow.nodes.iter().zip(&sizes).map(|(n, (w, h))| Node { width: *w, height: *h, group: n.group }).collect();
    let edges: Vec<Edge> = flow.edges.iter().map(|e| Edge { from: e.from, to: e.to, label: (!e.text.is_empty()).then(|| (widest(&e.text, SMALL, false) + 8.0, height_of(&e.text, SMALL) + 4.0)) }).collect();
    let gaps = if flow.groups.is_empty() { Gaps::default() } else { Gaps { node: 34.0, rank: 70.0 } };
    let layout = diagram::layered(&nodes, &edges, direction, &gaps);
    let mut scene = Scene::default();
    draw_groups(&mut scene, &flow.groups, &flow.nodes.iter().map(|n| n.group).collect::<Vec<_>>(), &layout.centres, &sizes);
    let shapes: Vec<Shape> = flow.nodes.iter().map(|n| n.shape).collect();
    for (n, edge) in flow.edges.iter().enumerate() {
        if edge.hidden {
            continue;
        }
        let width = if edge.thick { 2.4 } else { 1.2 };
        draw_route(&mut scene, &layout.routes[n], (&shapes, &sizes), (edge.from, edge.to), (edge.start, edge.end), width, edge.dashed);
        if let Some((x, y)) = layout.labels[n] {
            label(&mut scene, x, y, &edge.text);
        }
    }
    for (n, node) in flow.nodes.iter().enumerate() {
        let (cx, cy) = layout.centres[n];
        let (w, h) = sizes[n];
        diagram::shape(&mut scene, node.shape, cx - w / 2.0, cy - h / 2.0, w, h, Paint::Fill, Paint::Line);
        scene.lines(cx, cy, &node.text, SIZE, false, Paint::Text);
    }
    scene
}

/// Words on a line, with the page cleared behind them.
fn label(scene: &mut Scene, x: f32, y: f32, text: &[String]) {
    if text.is_empty() {
        return;
    }
    let (w, h) = (widest(text, SMALL, false) + 6.0, height_of(text, SMALL) + 2.0);
    scene.items.push(Item::Rect { x: x - w / 2.0, y: y - h / 2.0, w, h, radius: 2.0, fill: Paint::Back, stroke: Paint::None, dashed: false });
    scene.lines(x, y, text, SMALL, false, Paint::Text);
}

/// Draws a line along a route, cut back to the edges of the shapes it joins, with its heads.
fn draw_route(scene: &mut Scene, route: &[(f32, f32)], (shapes, sizes): (&[Shape], &[(f32, f32)]), (from, to): (usize, usize), (start, end): (Head, Head), width: f32, dashed: bool) {
    if route.len() < 2 {
        return;
    }
    let mut points = route.to_vec();
    // A line from a node to itself is drawn as it was routed; another is cut to the shapes' edges.
    if from != to {
        let last = points.len() - 1;
        let (fw, fh) = sizes[from];
        points[0] = diagram::edge_point(shapes[from], route[0].0, route[0].1, fw, fh, route[1]);
        let (tw, th) = sizes[to];
        points[last] = diagram::edge_point(shapes[to], route[last].0, route[last].1, tw, th, route[last - 1]);
    }
    let points = diagram::smooth(&points);
    let n = points.len();
    if n < 2 {
        return;
    }
    scene.line(points.clone(), Paint::Line, width, dashed);
    diagram::head(scene, end, points[n - 1], points[n - 2], Paint::Line);
    diagram::head(scene, start, points[0], points[1], Paint::Line);
}

/// Draws each group as a box round its nodes (and the groups inside it), its title at the top, outer ones first.
fn draw_groups(scene: &mut Scene, groups: &[Group], membership: &[Option<usize>], centres: &[(f32, f32)], sizes: &[(f32, f32)]) {
    let depth = |mut g: usize| {
        let mut d = 0;
        while let Some(p) = groups[g].parent {
            d += 1;
            g = p;
        }
        d
    };
    let inside = |node_group: Option<usize>, group: usize| {
        let mut at = node_group;
        while let Some(g) = at {
            if g == group {
                return true;
            }
            at = groups[g].parent;
        }
        false
    };
    let deepest = (0..groups.len()).map(depth).max().unwrap_or(0);
    let mut order: Vec<usize> = (0..groups.len()).collect();
    order.sort_by_key(|g| depth(*g));
    for g in order {
        let members: Vec<usize> = (0..membership.len()).filter(|n| inside(membership[*n], g)).collect();
        if members.is_empty() {
            continue;
        }
        let pad = 10.0 + 8.0 * (deepest - depth(g)) as f32;
        let left = members.iter().map(|n| centres[*n].0 - sizes[*n].0 / 2.0).fold(f32::MAX, f32::min) - pad;
        let right = members.iter().map(|n| centres[*n].0 + sizes[*n].0 / 2.0).fold(f32::MIN, f32::max) + pad;
        let top = members.iter().map(|n| centres[*n].1 - sizes[*n].1 / 2.0).fold(f32::MAX, f32::min) - pad - 18.0;
        let bottom = members.iter().map(|n| centres[*n].1 + sizes[*n].1 / 2.0).fold(f32::MIN, f32::max) + pad;
        let title_width = widest(&groups[g].title, SMALL, true) + 16.0;
        let (left, right) = if right - left < title_width { let grow = (title_width - (right - left)) / 2.0; (left - grow, right + grow) } else { (left, right) };
        scene.items.push(Item::Rect { x: left, y: top, w: right - left, h: bottom - top, radius: 3.0, fill: Paint::Group, stroke: Paint::Faint, dashed: false });
        scene.text((left + right) / 2.0, top + 11.0, groups[g].title.join(" "), SMALL, Align::Middle, true, Paint::Text);
    }
}

// ---------- Sequence diagrams ----------

#[derive(Clone, Debug)]
struct Participant {
    id: String,
    text: Vec<String>,
    actor: bool,
}

#[derive(Clone, Debug)]
enum Event {
    Message { from: usize, to: usize, text: Vec<String>, dashed: bool, head: Head, both: bool, activate: Option<bool> },
    Note { first: usize, last: usize, side: i8, text: Vec<String> },
    Activate(usize, bool),
    /// A frame starting (loop, alt, opt, par, critical, break, rect), a part of one (else, and, option), or the end.
    Open(String, Vec<String>),
    Part(Vec<String>),
    Close,
}

/// A sequence diagram's arrows, longest first, each with whether it's dotted, its head, and whether it has a
/// head at both ends.
const ARROWS: [(&str, bool, Head, bool); 10] = [
    ("<<-->>", true, Head::Arrow, true),
    ("<<->>", false, Head::Arrow, true),
    ("-->>", true, Head::Arrow, false),
    ("->>", false, Head::Arrow, false),
    ("--x", true, Head::Cross, false),
    ("-x", false, Head::Cross, false),
    ("--)", true, Head::Open, false),
    ("-)", false, Head::Open, false),
    ("-->", true, Head::None, false),
    ("->", false, Head::None, false),
];

fn sequence(lines: &[String]) -> Scene {
    let mut people: Vec<Participant> = Vec::new();
    let mut events: Vec<Event> = Vec::new();
    let mut numbered = false;
    let mut title: Vec<String> = Vec::new();
    let find = |people: &mut Vec<Participant>, id: &str| -> usize {
        let id = id.trim();
        if let Some(at) = people.iter().position(|p| p.id == id) {
            return at;
        }
        people.push(Participant { id: id.to_string(), text: vec![id.to_string()], actor: false });
        people.len() - 1
    };
    // Box groups are read but not drawn, so their ends mustn't close a frame.
    let mut boxes = 0;
    let mut frames = 0;
    for line in lines {
        let first_word = line.split_whitespace().next().unwrap_or("");
        let word = first_word.to_lowercase();
        let rest = line[first_word.len()..].trim();
        match word.as_str() {
            "participant" | "actor" => {
                let (id, alias) = match rest.split_once(" as ") {
                    Some((id, alias)) => (id.trim(), Some(words(alias))),
                    None => (rest, None),
                };
                let at = find(&mut people, id);
                if let Some(alias) = alias {
                    people[at].text = alias;
                }
                people[at].actor = word == "actor";
                continue;
            }
            "create" | "destroy" | "links" | "link" | "properties" | "details" => continue,
            "autonumber" => {
                numbered = true;
                continue;
            }
            "title" | "title:" => {
                title = words(rest.trim_start_matches(':'));
                continue;
            }
            "activate" | "deactivate" => {
                let at = find(&mut people, rest);
                events.push(Event::Activate(at, word == "activate"));
                continue;
            }
            "note" => {
                let (place, text) = rest.split_once(':').unwrap_or((rest, ""));
                let place_lower = place.to_lowercase();
                let (side, names) = if let Some(names) = place_lower.strip_prefix("right of") {
                    (1, &place[place.len() - names.len()..])
                } else if let Some(names) = place_lower.strip_prefix("left of") {
                    (-1, &place[place.len() - names.len()..])
                } else if let Some(names) = place_lower.strip_prefix("over") {
                    (0, &place[place.len() - names.len()..])
                } else {
                    continue;
                };
                let ids: Vec<usize> = names.split(',').map(|n| find(&mut people, n)).collect();
                let (first, last) = (*ids.iter().min().unwrap_or(&0), *ids.iter().max().unwrap_or(&0));
                events.push(Event::Note { first, last, side, text: words(text) });
                continue;
            }
            "box" => {
                boxes += 1;
                continue;
            }
            "loop" | "alt" | "opt" | "par" | "critical" | "break" | "rect" | "par_over" => {
                frames += 1;
                let shown = if word == "rect" { Vec::new() } else { words(rest) };
                events.push(Event::Open(word.clone(), shown));
                continue;
            }
            "else" | "and" | "option" => {
                events.push(Event::Part(words(rest)));
                continue;
            }
            "end" => {
                if frames > 0 {
                    frames -= 1;
                    events.push(Event::Close);
                } else {
                    boxes = (boxes - 1).max(0);
                }
                continue;
            }
            _ => {}
        }
        // A message: From->>To: words.
        let (head_part, text) = line.split_once(':').unwrap_or((line.as_str(), ""));
        let found = ARROWS.iter().filter_map(|arrow| head_part.find(arrow.0).map(|at| (at, arrow))).min_by_key(|(at, arrow)| (*at, std::cmp::Reverse(arrow.0.len())));
        let Some((at, (arrow, dashed, head, both))) = found else { continue };
        let from = head_part[..at].trim();
        let mut to = head_part[at + arrow.len()..].trim();
        let activate = if let Some(t) = to.strip_prefix('+') {
            to = t.trim();
            Some(true)
        } else if let Some(t) = to.strip_prefix('-') {
            to = t.trim();
            Some(false)
        } else {
            None
        };
        if from.is_empty() || to.is_empty() {
            continue;
        }
        let from = find(&mut people, from);
        let to = find(&mut people, to);
        events.push(Event::Message { from, to, text: words(text), dashed: *dashed, head: *head, both: *both, activate });
    }
    draw_sequence(&people, &events, numbered, &title)
}

fn draw_sequence(people: &[Participant], events: &[Event], numbered: bool, title: &[String]) -> Scene {
    let mut scene = Scene::default();
    if people.is_empty() {
        return scene;
    }
    let count = people.len();
    let boxes: Vec<(f32, f32)> = people.iter().map(|p| ((widest(&p.text, SIZE, false) + 24.0).max(70.0), height_of(&p.text, SIZE) + 16.0)).collect();
    let head_height = boxes.iter().map(|b| b.1).fold(0.0, f32::max).max(if people.iter().any(|p| p.actor) { 40.0 + SIZE * diagram::LINE } else { 0.0 });
    // The gaps between neighbours: enough for their boxes, then for whatever messages and notes span them.
    let mut gaps: Vec<f32> = (0..count.saturating_sub(1)).map(|n| boxes[n].0 / 2.0 + boxes[n + 1].0 / 2.0 + 24.0).collect();
    let widen = |gaps: &mut Vec<f32>, first: usize, last: usize, need: f32| {
        if first >= last || last > gaps.len() {
            return;
        }
        let have: f32 = gaps[first..last].iter().sum();
        if have < need {
            let more = (need - have) / (last - first) as f32;
            gaps[first..last].iter_mut().for_each(|g| *g += more);
        }
    };
    let mut right_margin: f32 = boxes[count - 1].0 / 2.0;
    let mut left_margin: f32 = boxes[0].0 / 2.0;
    for event in events {
        match event {
            Event::Message { from, to, text, .. } if from == to => {
                let need = widest(text, SMALL, false) + 50.0;
                if *from + 1 < count {
                    widen(&mut gaps, *from, from + 1, need);
                } else {
                    right_margin = right_margin.max(need);
                }
            }
            Event::Message { from, to, text, .. } => widen(&mut gaps, (*from).min(*to), (*from).max(*to), widest(text, SMALL, false) + 36.0),
            Event::Note { first, last, side, text } => {
                let need = widest(text, SMALL, false) + 24.0;
                match side {
                    1 if *first + 1 < count => widen(&mut gaps, *first, first + 1, need + 20.0),
                    1 => right_margin = right_margin.max(need + 20.0),
                    -1 if *first > 0 => widen(&mut gaps, first - 1, *first, need + 20.0),
                    -1 => left_margin = left_margin.max(need + 20.0),
                    _ => widen(&mut gaps, *first, *last, need - 20.0),
                }
            }
            _ => {}
        }
    }
    let mut xs = vec![left_margin];
    for gap in &gaps {
        xs.push(xs.last().copied().unwrap_or(0.0) + gap);
    }
    let mut y = 0.0;
    if !title.is_empty() {
        scene.lines(xs[0] + (xs[count - 1] - xs[0]) / 2.0, 8.0, title, SIZE + 2.0, true, Paint::Text);
        y += height_of(title, SIZE + 2.0) + 10.0;
    }
    let top = y;
    y += head_height + 20.0;
    // Activation bars open on each participant: where each started.
    let mut active: Vec<Vec<f32>> = vec![Vec::new(); count];
    let mut bars: Vec<(usize, usize, f32, f32)> = Vec::new();
    // Frames open: their kind, words, where they start, the participants inside, and their parts' dividers.
    struct Frame {
        words: Vec<String>,
        kind: String,
        top: f32,
        first: usize,
        last: usize,
        parts: Vec<(f32, Vec<String>)>,
    }
    let mut frames: Vec<Frame> = Vec::new();
    let mut finished: Vec<(Frame, f32, usize)> = Vec::new();
    let mut number = 0;
    let mut later: Vec<Item> = Vec::new();
    let side_of = |active: &Vec<Vec<f32>>, who: usize, going_right: bool| -> f32 {
        let depth = active[who].len() as f32;
        if depth == 0.0 { xs[who] } else if going_right { xs[who] + 5.0 + (depth - 1.0) * 4.0 } else { xs[who] - 5.0 + (depth - 1.0) * 4.0 }
    };
    let touch = |frames: &mut Vec<Frame>, a: usize, b: usize| {
        for frame in frames.iter_mut() {
            frame.first = frame.first.min(a.min(b));
            frame.last = frame.last.max(a.max(b));
        }
    };
    for event in events {
        match event {
            Event::Message { from, to, text, dashed, head, both, activate } => {
                touch(&mut frames, *from, *to);
                let words_height = if text.is_empty() { 0.0 } else { height_of(text, SMALL) };
                y += words_height + 8.0;
                number += 1;
                if from == to {
                    let x = side_of(&active, *from, true);
                    let points = vec![(x, y), (x + 34.0, y), (x + 34.0, y + 18.0), (x + 2.0, y + 18.0)];
                    later.push(Item::Path { points, closed: false, fill: Paint::None, stroke: Paint::Line, width: 1.2, dashed: *dashed });
                    let mut heads = Scene::default();
                    diagram::head(&mut heads, *head, (x + 1.0, y + 18.0), (x + 34.0, y + 18.0), Paint::Line);
                    later.extend(heads.items);
                    for (n, line) in text.iter().enumerate() {
                        later.push(Item::Text { x: x + 4.0, y: y - words_height + SMALL * diagram::LINE * (n as f32 + 0.5) - 4.0, text: line.clone(), size: SMALL, align: Align::Start, bold: false, paint: Paint::Text });
                    }
                    y += 26.0;
                } else {
                    let right = to > from;
                    let start = side_of(&active, *from, right);
                    if *activate == Some(true) {
                        active[*to].push(y);
                    }
                    let end = side_of(&active, *to, !right);
                    let tip = if right { end - 1.0 } else { end + 1.0 };
                    later.push(Item::Path { points: vec![(start, y), (tip, y)], closed: false, fill: Paint::None, stroke: Paint::Line, width: 1.2, dashed: *dashed });
                    let mut heads = Scene::default();
                    diagram::head(&mut heads, *head, (tip, y), (start, y), Paint::Line);
                    if *both {
                        diagram::head(&mut heads, *head, (start, y), (tip, y), Paint::Line);
                    }
                    later.extend(heads.items);
                    let middle = (start + end) / 2.0;
                    for (n, line) in text.iter().enumerate() {
                        later.push(Item::Text { x: middle, y: y - words_height + SMALL * diagram::LINE * (n as f32 + 0.5) - 4.0, text: line.clone(), size: SMALL, align: Align::Middle, bold: false, paint: Paint::Text });
                    }
                    if numbered {
                        later.push(Item::Ellipse { cx: start, cy: y, rx: 7.0, ry: 7.0, fill: Paint::Accent, stroke: Paint::Accent });
                        later.push(Item::Text { x: start, y, text: number.to_string(), size: 9.0, align: Align::Middle, bold: true, paint: Paint::Back });
                    }
                    if *activate == Some(false)
                        && let Some(began) = active[*from].pop()
                    {
                        bars.push((*from, active[*from].len(), began, y));
                    }
                    y += 14.0;
                }
            }
            Event::Note { first, last, side, text } => {
                touch(&mut frames, *first, *last);
                let w = widest(text, SMALL, false) + 16.0;
                let h = height_of(text, SMALL) + 10.0;
                let (left, width) = match side {
                    1 => (xs[*first] + 10.0, w),
                    -1 => (xs[*first] - 10.0 - w, w),
                    _ if first == last => (xs[*first] - w / 2.0, w),
                    _ => {
                        let span = xs[*last] - xs[*first] + 40.0;
                        let width = span.max(w);
                        ((xs[*first] + xs[*last]) / 2.0 - width / 2.0, width)
                    }
                };
                y += 6.0;
                later.push(Item::Rect { x: left, y, w: width, h, radius: 0.0, fill: Paint::Note, stroke: Paint::Faint, dashed: false });
                for (n, line) in text.iter().enumerate() {
                    later.push(Item::Text { x: left + width / 2.0, y: y + 5.0 + SMALL * diagram::LINE * (n as f32 + 0.5), text: line.clone(), size: SMALL, align: Align::Middle, bold: false, paint: Paint::Text });
                }
                y += h + 8.0;
            }
            Event::Activate(who, on) => {
                if *on {
                    active[*who].push(y);
                } else if let Some(began) = active[*who].pop() {
                    bars.push((*who, active[*who].len(), began, y));
                }
            }
            Event::Open(kind, words) => {
                y += 10.0;
                frames.push(Frame { words: words.clone(), kind: kind.clone(), top: y, first: usize::MAX, last: 0, parts: Vec::new() });
                y += 24.0 + if words.len() > 1 { height_of(&words[1..], SMALL) } else { 0.0 };
            }
            Event::Part(words) => {
                y += 8.0;
                if let Some(frame) = frames.last_mut() {
                    frame.parts.push((y, words.clone()));
                }
                y += 20.0;
            }
            Event::Close => {
                y += 10.0;
                if let Some(frame) = frames.pop() {
                    let depth = frames.len();
                    finished.push((frame, y, depth));
                }
            }
        }
    }
    // Frames left open close at the bottom.
    while let Some(frame) = frames.pop() {
        let depth = frames.len();
        finished.push((frame, y, depth));
    }
    y += 16.0;
    let bottom = y;
    // Lifelines first, so everything goes over them.
    for x in &xs {
        scene.line(vec![(*x, top + head_height), (*x, bottom)], Paint::Faint, 0.8, true);
    }
    for (frame, end, depth) in &finished {
        let (first, last) = if frame.first == usize::MAX { (0, count - 1) } else { (frame.first, frame.last) };
        let pad = (26.0 - 6.0 * *depth as f32).max(8.0);
        let left = xs[first] - pad - 40.0_f32.min(boxes[first].0 / 2.0);
        let right = xs[last] + pad + 40.0_f32.min(boxes[last].0 / 2.0);
        if frame.kind == "rect" {
            scene.items.insert(0, Item::Rect { x: left, y: frame.top, w: right - left, h: end - frame.top, radius: 0.0, fill: Paint::Group, stroke: Paint::None, dashed: false });
            continue;
        }
        scene.items.push(Item::Rect { x: left, y: frame.top, w: right - left, h: end - frame.top, radius: 0.0, fill: Paint::None, stroke: Paint::Line, dashed: false });
        let tab = diagram::text_width(&frame.kind, SMALL, true) + 14.0;
        scene.polygon(vec![(left, frame.top), (left + tab, frame.top), (left + tab, frame.top + 12.0), (left + tab - 6.0, frame.top + 18.0), (left, frame.top + 18.0)], Paint::Group, Paint::Line);
        scene.text(left + 6.0, frame.top + 9.0, frame.kind.clone(), SMALL, Align::Start, true, Paint::Text);
        if let Some(first_line) = frame.words.first() {
            scene.text((left + tab + right) / 2.0, frame.top + 10.0, format!("[{first_line}]"), SMALL, Align::Middle, false, Paint::Text);
        }
        for (part_y, words) in &frame.parts {
            scene.line(vec![(left, *part_y), (right, *part_y)], Paint::Line, 1.0, true);
            if let Some(first_line) = words.first() {
                scene.text((left + right) / 2.0, part_y + 10.0, format!("[{first_line}]"), SMALL, Align::Middle, false, Paint::Text);
            }
        }
    }
    for (who, depth, began, ended) in bars {
        let x = xs[who] - 5.0 + depth as f32 * 4.0;
        scene.items.push(Item::Rect { x, y: began - 4.0, w: 10.0, h: ended - began + 8.0, radius: 0.0, fill: Paint::Fill, stroke: Paint::Line, dashed: false });
    }
    // Activations still open run to the bottom.
    for (who, starts) in active.iter().enumerate() {
        for (depth, began) in starts.iter().enumerate() {
            let x = xs[who] - 5.0 + depth as f32 * 4.0;
            scene.items.push(Item::Rect { x, y: began - 4.0, w: 10.0, h: bottom - began, radius: 0.0, fill: Paint::Fill, stroke: Paint::Line, dashed: false });
        }
    }
    scene.items.extend(later);
    for (n, person) in people.iter().enumerate() {
        for at in [top, bottom] {
            draw_participant(&mut scene, person, xs[n], at, boxes[n], head_height);
        }
    }
    scene
}

fn draw_participant(scene: &mut Scene, person: &Participant, x: f32, top: f32, (w, h): (f32, f32), head_height: f32) {
    if person.actor {
        // A stick figure, with the name under it.
        let (cx, cy) = (x, top + 7.0);
        scene.items.push(Item::Ellipse { cx, cy, rx: 6.0, ry: 6.0, fill: Paint::Fill, stroke: Paint::Line });
        scene.line(vec![(cx, cy + 6.0), (cx, cy + 20.0)], Paint::Line, 1.2, false);
        scene.line(vec![(cx - 10.0, cy + 11.0), (cx + 10.0, cy + 11.0)], Paint::Line, 1.2, false);
        scene.line(vec![(cx - 8.0, cy + 30.0), (cx, cy + 20.0), (cx + 8.0, cy + 30.0)], Paint::Line, 1.2, false);
        scene.lines(x, top + 40.0 + height_of(&person.text, SIZE) / 2.0, &person.text, SIZE, false, Paint::Text);
        return;
    }
    let y = top + head_height - h;
    scene.items.push(Item::Rect { x: x - w / 2.0, y, w, h, radius: 3.0, fill: Paint::Fill, stroke: Paint::Line, dashed: false });
    scene.lines(x, y + h / 2.0, &person.text, SIZE, false, Paint::Text);
}

// ---------- State diagrams ----------

#[derive(Clone, Debug)]
struct StateNode {
    text: Vec<String>,
    description: Vec<String>,
    shape: Shape,
    group: Option<usize>,
}

fn state(lines: &[String]) -> Scene {
    let mut direction = Direction::Down;
    let mut nodes: Vec<StateNode> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut groups: Vec<Group> = Vec::new();
    let mut group_of_name: HashMap<String, usize> = HashMap::new();
    let mut edges: Vec<(usize, usize, Vec<String>)> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut in_note = false;
    // [*] is a separate start (or end) in each composite state.
    let node = |nodes: &mut Vec<StateNode>, index: &mut HashMap<String, usize>, name: &str, group: Option<usize>, end: bool| -> usize {
        let key = if name == "[*]" { format!("[*]{}{:?}", if end { "end" } else { "start" }, group) } else { name.to_string() };
        if let Some(at) = index.get(&key) {
            return *at;
        }
        let shape = match name {
            "[*]" if end => Shape::End,
            "[*]" => Shape::Start,
            _ => Shape::Round,
        };
        nodes.push(StateNode { text: vec![name.to_string()], description: Vec::new(), shape, group });
        index.insert(key, nodes.len() - 1);
        nodes.len() - 1
    };
    for line in lines {
        let lower = line.to_lowercase();
        if in_note {
            if lower.trim() == "end note" {
                in_note = false;
            }
            continue;
        }
        let group = stack.last().copied();
        if lower.starts_with("direction ") {
            if let Some(d) = Direction::from_mermaid(line[10..].trim()) {
                direction = d;
            }
            continue;
        }
        if lower.starts_with("note ") {
            in_note = !line.contains(':');
            continue;
        }
        if line == "}" {
            stack.pop();
            continue;
        }
        if line == "--" || lower.starts_with("classdef") || lower.starts_with("class ") || lower.starts_with("style ") || lower.starts_with("hide empty") || lower.starts_with("acctitle") || lower.starts_with("accdescr") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("state ") {
            let rest = rest.trim();
            let opens = rest.ends_with('{');
            let rest = rest.trim_end_matches('{').trim();
            // state "Words" as Name, state Name <<choice>>, state Name {.
            let (name, text) = if let Some(quoted) = rest.strip_prefix('"') {
                let (text, after) = quoted.split_once('"').unwrap_or((quoted, ""));
                (after.trim().trim_start_matches("as").trim().to_string(), Some(words(text)))
            } else if let Some((text, name)) = rest.split_once(" as ") {
                (name.trim().to_string(), Some(words(text)))
            } else {
                (rest.split_whitespace().next().unwrap_or("").to_string(), None)
            };
            if name.is_empty() {
                continue;
            }
            if opens {
                groups.push(Group { title: text.clone().unwrap_or_else(|| vec![name.clone()]), parent: group });
                group_of_name.insert(name.clone(), groups.len() - 1);
                stack.push(groups.len() - 1);
                continue;
            }
            let at = node(&mut nodes, &mut index, &name, group, false);
            if let Some(text) = text {
                nodes[at].text = text;
            }
            if rest.contains("<<choice>>") {
                nodes[at].shape = Shape::Diamond;
            } else if rest.contains("<<fork>>") || rest.contains("<<join>>") {
                nodes[at].shape = Shape::Bar;
            }
            continue;
        }
        if let Some(at) = line.find("-->") {
            let from = line[..at].trim();
            let (to, text) = line[at + 3..].split_once(':').map_or((line[at + 3..].trim(), Vec::new()), |(to, text)| (to.trim(), words(text)));
            // A transition to or from a composite state goes to its first state inside... or, simpler, to a
            // stand-in for it drawn as a state.
            let from = node(&mut nodes, &mut index, from, group, false);
            let to = node(&mut nodes, &mut index, to, group, true);
            edges.push((from, to, text));
            continue;
        }
        if let Some((name, text)) = line.split_once(':') {
            let at = node(&mut nodes, &mut index, name.trim(), group, false);
            nodes[at].description.extend(words(text));
            continue;
        }
        let name = line.trim();
        if !name.is_empty() && !name.contains(' ') {
            node(&mut nodes, &mut index, name, group, false);
        }
    }
    // A transition to a composite state's name: drawn to its box's stand-in, which sits in the group.
    for (name, g) in &group_of_name {
        if let Some(at) = index.get(name) {
            nodes[*at].group = Some(*g);
        }
    }
    let sizes: Vec<(f32, f32)> = nodes
        .iter()
        .map(|n| match n.shape {
            Shape::Round => {
                let w = widest(&n.text, SIZE, false).max(widest(&n.description, SMALL, false)) + 28.0;
                let h = height_of(&n.text, SIZE) + 16.0 + if n.description.is_empty() { 0.0 } else { height_of(&n.description, SMALL) + 8.0 };
                (w.max(50.0), h)
            }
            Shape::Diamond => (26.0, 26.0),
            Shape::Bar if matches!(direction, Direction::Right | Direction::Left) => (7.0, 60.0),
            other => shape_size(other, &n.text),
        })
        .collect();
    let layout_nodes: Vec<Node> = nodes.iter().zip(&sizes).map(|(n, (w, h))| Node { width: *w, height: *h, group: n.group }).collect();
    let layout_edges: Vec<Edge> = edges.iter().map(|(from, to, text)| Edge { from: *from, to: *to, label: (!text.is_empty()).then(|| (widest(text, SMALL, false) + 8.0, height_of(text, SMALL) + 4.0)) }).collect();
    let gaps = if groups.is_empty() { Gaps::default() } else { Gaps { node: 34.0, rank: 70.0 } };
    let layout = diagram::layered(&layout_nodes, &layout_edges, direction, &gaps);
    let mut scene = Scene::default();
    draw_groups(&mut scene, &groups, &nodes.iter().map(|n| n.group).collect::<Vec<_>>(), &layout.centres, &sizes);
    let shapes: Vec<Shape> = nodes.iter().map(|n| n.shape).collect();
    for (n, (from, to, text)) in edges.iter().enumerate() {
        draw_route(&mut scene, &layout.routes[n], (&shapes, &sizes), (*from, *to), (Head::None, Head::Arrow), 1.2, false);
        if let Some((x, y)) = layout.labels[n] {
            label(&mut scene, x, y, text);
        }
    }
    for (n, state) in nodes.iter().enumerate() {
        let (cx, cy) = layout.centres[n];
        let (w, h) = sizes[n];
        let (x, y) = (cx - w / 2.0, cy - h / 2.0);
        if state.shape != Shape::Round {
            diagram::shape(&mut scene, state.shape, x, y, w, h, Paint::Fill, Paint::Line);
            continue;
        }
        scene.items.push(Item::Rect { x, y, w, h, radius: 8.0, fill: Paint::Fill, stroke: Paint::Line, dashed: false });
        if state.description.is_empty() {
            scene.lines(cx, cy, &state.text, SIZE, false, Paint::Text);
        } else {
            let title_h = height_of(&state.text, SIZE) + 10.0;
            scene.lines(cx, y + title_h / 2.0 + 2.0, &state.text, SIZE, false, Paint::Text);
            scene.line(vec![(x, y + title_h + 2.0), (x + w, y + title_h + 2.0)], Paint::Line, 1.0, false);
            scene.lines(cx, y + title_h + 4.0 + (h - title_h - 4.0) / 2.0, &state.description, SMALL, false, Paint::Text);
        }
    }
    scene
}

// ---------- Class diagrams ----------

#[derive(Clone, Debug, Default)]
struct Class {
    name: String,
    annotation: Option<String>,
    fields: Vec<String>,
    methods: Vec<String>,
}

/// A relation's line as Mermaid writes it, a mark at either end: <|-- , *-- , o-- , --> , ..|> and the like.
fn relation_token(token: &str) -> Option<(Head, Head, bool)> {
    for line in ["--", ".."] {
        let Some(at) = token.find(line) else { continue };
        let left = match &token[..at] {
            "" => Head::None,
            "<|" => Head::Triangle,
            "*" => Head::Diamond(true),
            "o" => Head::Diamond(false),
            "<" => Head::Arrow,
            _ => return None,
        };
        let right = match &token[at + line.len()..] {
            "" => Head::None,
            "|>" => Head::Triangle,
            "*" => Head::Diamond(true),
            "o" => Head::Diamond(false),
            ">" => Head::Arrow,
            _ => return None,
        };
        return Some((left, right, line == ".."));
    }
    None
}

/// The relation in a line: which of its words is the line, and its ends.
fn relation(text: &str) -> Option<(usize, Head, Head, bool)> {
    text.split_whitespace().enumerate().find_map(|(n, token)| relation_token(token).map(|(l, r, d)| (n, l, r, d)))
}

fn class(lines: &[String]) -> Scene {
    let mut direction = Direction::Down;
    let mut classes: Vec<Class> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    // (from, to, start head, end head, dashed, words, cardinality at each end)
    let mut relations: Vec<(usize, usize, Head, Head, bool, Vec<String>, String, String)> = Vec::new();
    let mut inside: Option<usize> = None;
    let get = |classes: &mut Vec<Class>, index: &mut HashMap<String, usize>, name: &str| -> usize {
        // A generic's ~T~ is shown as <T>.
        let name = name.trim().trim_matches('`');
        let shown = match name.split_once('~') {
            Some((base, rest)) => format!("{base}<{}>", rest.trim_end_matches('~')),
            None => name.to_string(),
        };
        let key = name.split('~').next().unwrap_or(name).to_string();
        if let Some(at) = index.get(&key) {
            return *at;
        }
        classes.push(Class { name: shown, ..Class::default() });
        index.insert(key, classes.len() - 1);
        classes.len() - 1
    };
    let member = |class: &mut Class, text: &str| {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if let Some(annotation) = text.strip_prefix("<<").and_then(|t| t.strip_suffix(">>")) {
            class.annotation = Some(annotation.to_string());
        } else if text.contains('(') {
            class.methods.push(text.replace('~', "<").to_string());
        } else {
            class.fields.push(text.replace('~', "<").to_string());
        }
    };
    for line in lines {
        if let Some(at) = inside {
            if line.starts_with('}') {
                inside = None;
            } else {
                member(&mut classes[at], line);
            }
            continue;
        }
        let lower = line.to_lowercase();
        if let Some(d) = lower.strip_prefix("direction ") {
            direction = Direction::from_mermaid(d.trim()).unwrap_or(direction);
            continue;
        }
        if lower.starts_with("note") || lower.starts_with("classdef") || lower.starts_with("style") || lower.starts_with("cssclass") || lower.starts_with("click") || lower.starts_with("link") || lower.starts_with("callback") || lower.starts_with("namespace") || line == "}" {
            continue;
        }
        if let Some(rest) = line.strip_prefix("class ") {
            let rest = rest.trim();
            let opens = rest.ends_with('{');
            let name = rest.trim_end_matches('{').trim();
            let name = name.split(":::").next().unwrap_or(name);
            let (name, label) = match name.split_once('[') {
                Some((name, label)) => (name.trim(), Some(words(label.trim_end_matches(']')))),
                None => (name, None),
            };
            let at = get(&mut classes, &mut index, name);
            if let Some(label) = label {
                classes[at].name = label.join(" ");
            }
            if opens {
                inside = Some(at);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("<<") {
            if let Some((annotation, name)) = rest.split_once(">>") {
                let at = get(&mut classes, &mut index, name);
                classes[at].annotation = Some(annotation.trim().to_string());
            }
            continue;
        }
        let (relation_part, text) = line.split_once(':').map_or((line.as_str(), Vec::new()), |(r, t)| (r, words(t)));
        if let Some((at, left, right, dashed)) = relation(relation_part) {
            let tokens: Vec<&str> = relation_part.split_whitespace().collect();
            // Cardinality in quotes beside each end: A "1" --> "*" B.
            let side = |part: &[&str]| -> (String, String) {
                let name = part.iter().filter(|t| !t.starts_with('"')).copied().collect::<Vec<_>>().join(" ");
                let card = part.iter().find(|t| t.starts_with('"')).map_or(String::new(), |t| t.trim_matches('"').to_string());
                (name, card)
            };
            let (from_name, from_card) = side(&tokens[..at]);
            let (to_name, to_card) = side(&tokens[at + 1..]);
            if from_name.is_empty() || to_name.is_empty() {
                continue;
            }
            let from = get(&mut classes, &mut index, &from_name);
            let to = get(&mut classes, &mut index, &to_name);
            relations.push((from, to, left, right, dashed, text, from_card, to_card));
            continue;
        }
        if let Some((name, text)) = line.split_once(':') {
            let name = name.trim();
            if !name.is_empty() && !name.contains(' ') {
                let at = get(&mut classes, &mut index, name);
                member(&mut classes[at], text);
            }
            continue;
        }
        if !line.contains(' ') {
            get(&mut classes, &mut index, line);
        }
    }
    // Each class's box: its name (with its annotation above), its fields, then its methods.
    let sizes: Vec<(f32, f32)> = classes
        .iter()
        .map(|c| {
            let mut w = diagram::text_width(&c.name, SIZE, true);
            if let Some(a) = &c.annotation {
                w = w.max(diagram::text_width(&format!("\u{AB}{a}\u{BB}"), SMALL, false));
            }
            w = w.max(widest(&c.fields, SMALL, false)).max(widest(&c.methods, SMALL, false));
            let head = SIZE * diagram::LINE + 12.0 + if c.annotation.is_some() { SMALL * diagram::LINE } else { 0.0 };
            let body = (c.fields.len().max(1) + c.methods.len().max(1)) as f32 * SMALL * diagram::LINE + 16.0;
            ((w + 24.0).max(80.0), head + body)
        })
        .collect();
    let nodes: Vec<Node> = sizes.iter().map(|(w, h)| Node { width: *w, height: *h, group: None }).collect();
    let edges: Vec<Edge> = relations.iter().map(|r| Edge { from: r.0, to: r.1, label: (!r.5.is_empty()).then(|| (widest(&r.5, SMALL, false) + 8.0, height_of(&r.5, SMALL) + 4.0)) }).collect();
    let layout = diagram::layered(&nodes, &edges, direction, &Gaps { node: 40.0, rank: 70.0 });
    let mut scene = Scene::default();
    let shapes = vec![Shape::Rect; classes.len()];
    for (n, (from, to, left, right, dashed, text, from_card, to_card)) in relations.iter().enumerate() {
        draw_route(&mut scene, &layout.routes[n], (&shapes, &sizes), (*from, *to), (*left, *right), 1.2, *dashed);
        if let Some((x, y)) = layout.labels[n] {
            label(&mut scene, x, y, text);
        }
        // Cardinalities just off each end.
        let route = &layout.routes[n];
        if route.len() >= 2 {
            for (card, end, toward, node) in [(from_card, route[0], route[1], *from), (to_card, route[route.len() - 1], route[route.len() - 2], *to)] {
                if card.is_empty() {
                    continue;
                }
                let (w, h) = sizes[node];
                let (ex, ey) = diagram::edge_point(Shape::Rect, end.0, end.1, w, h, toward);
                let (dx, dy) = (toward.0 - ex, toward.1 - ey);
                let length = (dx * dx + dy * dy).sqrt().max(1.0);
                scene.text(ex + dx / length * 14.0 + 10.0, ey + dy / length * 14.0, card.clone(), SMALL, Align::Start, false, Paint::Text);
            }
        }
    }
    for (n, c) in classes.iter().enumerate() {
        let (cx, cy) = layout.centres[n];
        let (w, h) = sizes[n];
        let (x, mut y) = (cx - w / 2.0, cy - h / 2.0);
        scene.items.push(Item::Rect { x, y, w, h, radius: 0.0, fill: Paint::Fill, stroke: Paint::Line, dashed: false });
        y += 6.0;
        if let Some(a) = &c.annotation {
            scene.text(cx, y + SMALL * 0.65, format!("\u{AB}{a}\u{BB}"), SMALL, Align::Middle, false, Paint::Text);
            y += SMALL * diagram::LINE;
        }
        scene.text(cx, y + SIZE * 0.65, c.name.clone(), SIZE, Align::Middle, true, Paint::Text);
        y += SIZE * diagram::LINE + 6.0;
        for (part, list) in [(0, &c.fields), (1, &c.methods)] {
            scene.line(vec![(x, y), (x + w, y)], Paint::Line, 1.0, false);
            y += 4.0;
            for item in list.iter() {
                scene.text(x + 8.0, y + SMALL * 0.65, item.clone(), SMALL, Align::Start, false, Paint::Text);
                y += SMALL * diagram::LINE;
            }
            if list.is_empty() {
                y += SMALL * diagram::LINE;
            }
            y += 4.0;
            let _ = part;
        }
    }
    scene
}

// ---------- Entity-relationship diagrams ----------

/// A crow's-foot end: the most (one or many) and whether none is allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cardinality {
    many: bool,
    optional: bool,
}

fn cardinality(mark: &str) -> Option<Cardinality> {
    Some(match mark {
        "|o" | "o|" => Cardinality { many: false, optional: true },
        "||" => Cardinality { many: false, optional: false },
        "}o" | "o{" => Cardinality { many: true, optional: true },
        "}|" | "|{" => Cardinality { many: true, optional: false },
        _ => return None,
    })
}

fn entities(lines: &[String]) -> Scene {
    let mut names: Vec<String> = Vec::new();
    let mut attributes: Vec<Vec<(String, String, String)>> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut relations: Vec<(usize, usize, Cardinality, Cardinality, bool, Vec<String>)> = Vec::new();
    let mut inside: Option<usize> = None;
    let mut get = |names: &mut Vec<String>, attributes: &mut Vec<Vec<(String, String, String)>>, name: &str| -> usize {
        let name = name.trim().trim_matches('"');
        if let Some(at) = index.get(name) {
            return *at;
        }
        names.push(name.to_string());
        attributes.push(Vec::new());
        index.insert(name.to_string(), names.len() - 1);
        names.len() - 1
    };
    for line in lines {
        if let Some(at) = inside {
            if line.starts_with('}') {
                inside = None;
                continue;
            }
            let mut parts = line.split_whitespace();
            let kind = parts.next().unwrap_or("").to_string();
            let name = parts.next().unwrap_or("").to_string();
            let keys: Vec<&str> = parts.take_while(|p| !p.starts_with('"')).collect();
            attributes[at].push((kind, name, keys.join(",")));
            continue;
        }
        if let Some(name) = line.strip_suffix('{') {
            inside = Some(get(&mut names, &mut attributes, name.trim()));
            continue;
        }
        // A relation: FIRST ||--o{ SECOND : words.
        let (relation, text) = line.split_once(':').map_or((line.as_str(), ""), |(r, t)| (r, t));
        let parts: Vec<&str> = relation.split_whitespace().collect();
        if parts.len() == 3 && parts[1].len() >= 6 {
            let marks = parts[1];
            let (left, rest) = marks.split_at(2);
            let (line_part, right) = rest.split_at(rest.len() - 2);
            if let (Some(left), Some(right)) = (cardinality(left), cardinality(right)) {
                let from = get(&mut names, &mut attributes, parts[0]);
                let to = get(&mut names, &mut attributes, parts[2]);
                relations.push((from, to, left, right, line_part == "..", words(text)));
                continue;
            }
        }
        if parts.len() == 1 {
            get(&mut names, &mut attributes, parts[0]);
        }
    }
    let row = SMALL * diagram::LINE + 2.0;
    let sizes: Vec<(f32, f32)> = names
        .iter()
        .zip(&attributes)
        .map(|(name, attrs)| {
            let columns = attrs.iter().fold((0.0_f32, 0.0_f32, 0.0_f32), |(a, b, c), (kind, n, keys)| (a.max(diagram::text_width(kind, SMALL, false)), b.max(diagram::text_width(n, SMALL, false)), c.max(diagram::text_width(keys, SMALL, false))));
            let w = (columns.0 + columns.1 + columns.2 + 40.0).max(diagram::text_width(name, SIZE, true) + 24.0).max(80.0);
            (w, SIZE * diagram::LINE + 14.0 + attrs.len() as f32 * row + if attrs.is_empty() { 0.0 } else { 6.0 })
        })
        .collect();
    let nodes: Vec<Node> = sizes.iter().map(|(w, h)| Node { width: *w, height: *h, group: None }).collect();
    let edges: Vec<Edge> = relations.iter().map(|r| Edge { from: r.0, to: r.1, label: (!r.5.is_empty()).then(|| (widest(&r.5, SMALL, false) + 8.0, height_of(&r.5, SMALL) + 4.0)) }).collect();
    let layout = diagram::layered(&nodes, &edges, Direction::Down, &Gaps { node: 50.0, rank: 70.0 });
    let mut scene = Scene::default();
    let shapes = vec![Shape::Rect; names.len()];
    for (n, (from, to, left, right, dashed, text)) in relations.iter().enumerate() {
        let route = &layout.routes[n];
        draw_route(&mut scene, route, (&shapes, &sizes), (*from, *to), (Head::None, Head::None), 1.2, *dashed);
        if route.len() >= 2 {
            let ends = [(*left, route[0], route[1], *from), (*right, route[route.len() - 1], route[route.len() - 2], *to)];
            for (card, end, toward, node) in ends {
                let (w, h) = sizes[node];
                let tip = diagram::edge_point(Shape::Rect, end.0, end.1, w, h, toward);
                crow_foot(&mut scene, card, tip, toward);
            }
        }
        if let Some((x, y)) = layout.labels[n] {
            label(&mut scene, x, y, text);
        }
    }
    for (n, name) in names.iter().enumerate() {
        let (cx, cy) = layout.centres[n];
        let (w, h) = sizes[n];
        let (x, y) = (cx - w / 2.0, cy - h / 2.0);
        scene.items.push(Item::Rect { x, y, w, h, radius: 0.0, fill: Paint::Fill, stroke: Paint::Line, dashed: false });
        let head = SIZE * diagram::LINE + 14.0;
        scene.text(cx, y + head / 2.0, name.clone(), SIZE, Align::Middle, true, Paint::Text);
        if attributes[n].is_empty() {
            continue;
        }
        scene.line(vec![(x, y + head), (x + w, y + head)], Paint::Line, 1.0, false);
        let kind_w = attributes[n].iter().map(|a| diagram::text_width(&a.0, SMALL, false)).fold(0.0, f32::max);
        let name_w = attributes[n].iter().map(|a| diagram::text_width(&a.1, SMALL, false)).fold(0.0, f32::max);
        for (k, (kind, attr, keys)) in attributes[n].iter().enumerate() {
            let row_y = y + head + 3.0 + row * (k as f32 + 0.5);
            scene.text(x + 8.0, row_y, kind.clone(), SMALL, Align::Start, false, Paint::Faint);
            scene.text(x + 16.0 + kind_w, row_y, attr.clone(), SMALL, Align::Start, false, Paint::Text);
            scene.text(x + 24.0 + kind_w + name_w, row_y, keys.clone(), SMALL, Align::Start, true, Paint::Accent);
        }
    }
    scene
}

/// A crow's-foot mark where a line meets an entity: a bar for one, a fork for many, a ring when none will do.
fn crow_foot(scene: &mut Scene, card: Cardinality, tip: (f32, f32), toward: (f32, f32)) {
    let (dx, dy) = (toward.0 - tip.0, toward.1 - tip.1);
    let length = (dx * dx + dy * dy).sqrt().max(0.001);
    let (ux, uy) = (dx / length, dy / length);
    let (px, py) = (-uy, ux);
    let at = |along: f32, side: f32| (tip.0 + ux * along + px * side, tip.1 + uy * along + py * side);
    if card.many {
        scene.line(vec![at(0.0, -7.0), at(10.0, 0.0), at(0.0, 7.0)], Paint::Line, 1.2, false);
    } else {
        scene.line(vec![at(6.0, -6.0), at(6.0, 6.0)], Paint::Line, 1.2, false);
    }
    if card.optional {
        let (cx, cy) = at(17.0, 0.0);
        scene.items.push(Item::Ellipse { cx, cy, rx: 4.0, ry: 4.0, fill: Paint::Back, stroke: Paint::Line });
    } else {
        scene.line(vec![at(13.0, -6.0), at(13.0, 6.0)], Paint::Line, 1.2, false);
    }
}

// ---------- Pie charts ----------

fn pie(first: &str, lines: &[String]) -> Scene {
    let mut title = first.split_once("title").map(|(_, t)| t.trim().to_string()).unwrap_or_default();
    let show_data = first.contains("showData");
    let mut slices: Vec<(String, f64)> = Vec::new();
    for line in lines {
        if let Some(t) = line.strip_prefix("title") {
            title = t.trim().to_string();
            continue;
        }
        if line.trim() == "showData" {
            continue;
        }
        if let Some((name, value)) = line.rsplit_once(':')
            && let Ok(value) = value.trim().parse::<f64>()
            && value > 0.0
        {
            slices.push((words(name).join(" "), value));
        }
    }
    let mut scene = Scene::default();
    let total: f64 = slices.iter().map(|(_, v)| v).sum();
    if total <= 0.0 {
        return scene;
    }
    let radius = 110.0;
    let (cx, cy) = (radius + 10.0, radius + if title.is_empty() { 10.0 } else { 40.0 });
    if !title.is_empty() {
        scene.text(cx, 14.0, title, SIZE + 2.0, Align::Middle, true, Paint::Text);
    }
    // Slices go clockwise from the top, as Mermaid's do.
    let mut angle = -90.0_f32;
    for (n, (_, value)) in slices.iter().enumerate() {
        let sweep = (*value / total * 360.0) as f32;
        let mut points = vec![(cx, cy)];
        points.extend(diagram::arc(cx, cy, radius, radius, angle, angle + sweep));
        if sweep >= 359.9 {
            points.remove(0);
        }
        scene.polygon(points, Paint::Series(n as u8), Paint::Back);
        let middle = (angle + sweep / 2.0).to_radians();
        let share = *value / total * 100.0;
        if sweep > 14.0 {
            scene.text(cx + radius * 0.65 * middle.cos(), cy + radius * 0.65 * middle.sin(), format!("{share:.0}%"), SMALL, Align::Middle, true, Paint::Back);
        }
        angle += sweep;
    }
    let mut y = cy - slices.len() as f32 * 10.0;
    for (n, (name, value)) in slices.iter().enumerate() {
        let x = cx + radius + 24.0;
        scene.items.push(Item::Rect { x, y: y - 6.0, w: 12.0, h: 12.0, radius: 2.0, fill: Paint::Series(n as u8), stroke: Paint::None, dashed: false });
        let words = if show_data { format!("{name} [{}]", trim_number(*value)) } else { name.clone() };
        scene.text(x + 18.0, y, words, SMALL, Align::Start, false, Paint::Text);
        y += 20.0;
    }
    scene
}

fn trim_number(value: f64) -> String {
    if value.fract() == 0.0 { format!("{value:.0}") } else { format!("{value}") }
}

// ---------- Gantt charts ----------

#[derive(Clone, Debug)]
struct Task {
    name: String,
    section: usize,
    start: f64,
    end: f64,
    done: bool,
    active: bool,
    critical: bool,
    milestone: bool,
}

/// A date as days from 1 January 1970, from YYYY-MM-DD with perhaps a time after it.
fn day_number(text: &str) -> Option<f64> {
    let text = text.trim();
    let (date, time) = text.split_once([' ', 'T']).map_or((text, ""), |(d, t)| (d, t));
    let date = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let days = (date - chrono::NaiveDate::from_ymd_opt(1970, 1, 1)?).num_days() as f64;
    let hours = time.split(':').next().and_then(|h| h.parse::<f64>().ok()).unwrap_or(0.0);
    Some(days + hours / 24.0)
}

/// A length of time, as days: 3d, 2w, 12h.
fn duration(text: &str) -> Option<f64> {
    let text = text.trim();
    let unit = text.chars().last()?;
    let number: f64 = text[..text.len() - unit.len_utf8()].trim().parse().ok()?;
    Some(match unit {
        'd' => number,
        'w' => number * 7.0,
        'h' => number / 24.0,
        'm' => number / 1440.0,
        's' => number / 86400.0,
        _ => return None,
    })
}

fn gantt(lines: &[String]) -> Result<Scene, String> {
    let mut title = String::new();
    let mut sections: Vec<String> = Vec::new();
    let mut tasks: Vec<Task> = Vec::new();
    let mut ids: HashMap<String, usize> = HashMap::new();
    for line in lines {
        let lower = line.to_lowercase();
        if let Some(t) = line.strip_prefix("title") {
            title = t.trim().to_string();
            continue;
        }
        if let Some(s) = line.strip_prefix("section") {
            sections.push(s.trim().to_string());
            continue;
        }
        if ["dateformat", "axisformat", "excludes", "includes", "todaymarker", "tickinterval", "weekday", "inclusiveenddates", "topaxis", "displaymode", "acctitle", "accdescr", "click"].iter().any(|w| lower.starts_with(w)) {
            continue;
        }
        let Some((name, data)) = line.split_once(':') else { continue };
        let mut parts: Vec<&str> = data.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
        let mut task = Task { name: name.trim().to_string(), section: sections.len().saturating_sub(1), start: 0.0, end: 0.0, done: false, active: false, critical: false, milestone: false };
        while let Some(first) = parts.first() {
            match *first {
                "done" => task.done = true,
                "active" => task.active = true,
                "crit" => task.critical = true,
                "milestone" => task.milestone = true,
                _ => break,
            }
            parts.remove(0);
        }
        // What's left: [id,] [start,] end. With one, it's the length, after the task before.
        let previous_end = tasks.last().map(|t: &Task| t.end);
        let id = (parts.len() == 3).then(|| parts.remove(0).to_string());
        let start_text = if parts.len() == 2 { Some(parts.remove(0)) } else { None };
        let start = match start_text {
            Some(text) if text.starts_with("after ") => text[6..].split_whitespace().filter_map(|id| ids.get(id)).map(|n| tasks[*n].end).fold(None, |a: Option<f64>, b| Some(a.map_or(b, |a| a.max(b)))),
            Some(text) => day_number(text),
            None => previous_end,
        };
        let Some(start) = start else {
            return Err(crate::i18n::tf!("This Gantt chart's task \u{201C}{}\u{201D} has a start that can't be read: dates are read as YYYY-MM-DD here.", task.name));
        };
        let end_text = parts.first().copied().unwrap_or("1d");
        let end = duration(end_text).map(|d| start + d).or_else(|| day_number(end_text).map(|d| d + 1.0)).unwrap_or(start + 1.0);
        task.start = start;
        task.end = if task.milestone { start } else { end.max(start) };
        if let Some(id) = id {
            ids.insert(id, tasks.len());
        }
        tasks.push(task);
    }
    let mut scene = Scene::default();
    if tasks.is_empty() {
        return Ok(scene);
    }
    let first_day = tasks.iter().map(|t| t.start).fold(f64::MAX, f64::min).floor();
    let last_day = tasks.iter().map(|t| t.end).fold(f64::MIN, f64::max).ceil().max(first_day + 1.0);
    let days = last_day - first_day;
    let scale = (440.0 / days as f32).clamp(1.5, 40.0);
    let label_w = tasks.iter().map(|t| diagram::text_width(&t.name, SMALL, false)).fold(0.0, f32::max).max(sections.iter().map(|s| diagram::text_width(s, SMALL, true)).fold(0.0, f32::max)) + 20.0;
    let row = 22.0;
    let mut y = 0.0;
    if !title.is_empty() {
        scene.text(label_w + days as f32 * scale / 2.0, 10.0, title, SIZE + 2.0, Align::Middle, true, Paint::Text);
        y += 30.0;
    }
    let x_of = |day: f64| label_w + (day - first_day) as f32 * scale;
    let chart_top = y;
    // Sections, each with its tasks under it on a shaded band, every other one.
    let mut rows: Vec<(f32, &Task)> = Vec::new();
    let section_count = sections.len().max(1);
    for section in 0..section_count {
        let in_section: Vec<&Task> = tasks.iter().filter(|t| t.section == section).collect();
        if in_section.is_empty() {
            continue;
        }
        let band_h = in_section.len() as f32 * row;
        if section % 2 == 0 {
            scene.items.push(Item::Rect { x: 0.0, y, w: x_of(last_day), h: band_h, radius: 0.0, fill: Paint::Group, stroke: Paint::None, dashed: false });
        }
        if let Some(name) = sections.get(section) {
            scene.text(4.0, y + 9.0, name.clone(), SMALL, Align::Start, true, Paint::Text);
        }
        for task in in_section {
            rows.push((y, task));
            y += row;
        }
    }
    let chart_bottom = y;
    // The axis: a tick a day, a week or a month apart, whichever leaves room for the dates.
    let step = [1.0, 2.0, 7.0, 14.0, 30.0, 61.0, 91.0, 182.0, 365.0].into_iter().find(|step| *step as f32 * scale >= 70.0).unwrap_or(365.0);
    let mut day = first_day;
    let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or_default();
    while day <= last_day + 0.001 {
        let x = x_of(day);
        scene.line(vec![(x, chart_top), (x, chart_bottom)], Paint::Faint, 0.5, true);
        let date = epoch + chrono::Duration::days(day as i64);
        scene.text(x, chart_bottom + 10.0, date.format("%Y-%m-%d").to_string(), 10.0, Align::Middle, false, Paint::Faint);
        day += step;
    }
    for (row_y, task) in rows {
        let (x0, x1) = (x_of(task.start), x_of(task.end));
        let middle = row_y + row / 2.0;
        let indent = if sections.is_empty() { 4.0 } else { 14.0 };
        let _ = indent;
        if task.milestone {
            scene.polygon(vec![(x0, middle - 7.0), (x0 + 7.0, middle), (x0, middle + 7.0), (x0 - 7.0, middle)], Paint::Accent, Paint::Line);
            scene.text(x0 + 12.0, middle, task.name.clone(), SMALL, Align::Start, false, Paint::Text);
            continue;
        }
        let fill = if task.critical { Paint::Warning } else if task.done { Paint::Faint } else if task.active { Paint::Accent } else { Paint::Series(0) };
        scene.items.push(Item::Rect { x: x0, y: row_y + 3.0, w: (x1 - x0).max(2.0), h: row - 6.0, radius: 3.0, fill, stroke: Paint::None, dashed: false });
        let width = diagram::text_width(&task.name, SMALL, false);
        // The name inside the bar if it fits, after it if not.
        if width + 8.0 < x1 - x0 {
            scene.text((x0 + x1) / 2.0, middle, task.name.clone(), SMALL, Align::Middle, false, Paint::Back);
        } else {
            scene.text(x1 + 5.0, middle, task.name.clone(), SMALL, Align::Start, false, Paint::Text);
        }
    }
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxes(scene: &Scene) -> usize {
        scene.items.iter().filter(|i| matches!(i, Item::Rect { fill: Paint::Fill, .. } | Item::Path { closed: true, fill: Paint::Fill, .. } | Item::Ellipse { fill: Paint::Fill, .. })).count()
    }

    #[test]
    fn a_flowchart_has_its_nodes_shapes_and_labelled_links() {
        let source = "flowchart LR\n  A[Start] --> B{Is it?}\n  B -->|Yes| C(Round)\n  B -- No --> D((Circle))\n  C & D --> E[(Store)]\n  %% a comment\n  E -.-> A; E ==> F>Flag]";
        let flow_lines = statements(source);
        let mut flow = Flow::default();
        let first = &flow_lines[0];
        assert!(first.starts_with("flowchart"));
        for line in &flow_lines[1..] {
            let Some((mut sources, mut used)) = read_nodes(&mut flow, line, None) else { continue };
            while let Some((start, end, dashed, thick, hidden, text, took)) = read_link(&line[used..]) {
                used += took;
                let Some((targets, took)) = read_nodes(&mut flow, &line[used..], None) else { break };
                used += took;
                for from in &sources {
                    for to in &targets {
                        flow.edges.push(FlowEdge { from: *from, to: *to, text: text.clone(), start, end, dashed, thick, hidden });
                    }
                }
                sources = targets;
            }
        }
        let mut names: Vec<(&usize, &str)> = flow.index.iter().map(|(id, n)| (n, id.as_str())).collect();
        names.sort();
        let names: Vec<&str> = names.into_iter().map(|(_, id)| id).collect();
        assert_eq!(names, ["A", "B", "C", "D", "E", "F"]);
        let shape = |id: &str| flow.nodes[flow.index[id]].shape;
        assert_eq!(shape("A"), Shape::Rect);
        assert_eq!(shape("B"), Shape::Diamond);
        assert_eq!(shape("C"), Shape::Round);
        assert_eq!(shape("D"), Shape::Circle);
        assert_eq!(shape("E"), Shape::Cylinder);
        assert_eq!(shape("F"), Shape::Flag);
        assert_eq!(flow.nodes[flow.index["B"]].text, ["Is it?"]);
        assert_eq!(flow.edges.len(), 7, "{:?}", flow.edges);
        assert_eq!(flow.edges[1].text, ["Yes"]);
        assert_eq!(flow.edges[2].text, ["No"]);
        assert!(flow.edges.iter().any(|e| e.dashed && e.from == flow.index["E"] && e.to == flow.index["A"]));
        assert!(flow.edges.iter().any(|e| e.thick && e.to == flow.index["F"]));
        let scene = render(source).unwrap();
        for word in ["Start", "Is it?", "Yes", "No", "Round", "Circle", "Store", "Flag"] {
            assert!(scene.words().contains(&word), "{word} is drawn");
        }
        assert!(scene.width > 100.0 && scene.height > 50.0);
    }

    #[test]
    fn node_words_can_be_quoted_and_broken_into_lines() {
        let (id, shaped, used) = read_node("n1[\"Words [with] brackets<br>and a line\"] --> x").unwrap();
        assert_eq!(id, "n1");
        let (words, shape) = shaped.unwrap();
        assert_eq!(words, ["Words [with] brackets", "and a line"]);
        assert_eq!(shape, Shape::Rect);
        assert_eq!(&"n1[\"Words [with] brackets<br>and a line\"] --> x"[used..], " --> x");
        assert_eq!(read_node("a[/lean/]").unwrap().1.unwrap().1, Shape::Lean(true));
        assert_eq!(read_node("a[/trap\\]").unwrap().1.unwrap().1, Shape::Trapezoid(true));
        assert_eq!(read_node("a{{hex}}:::big --> b").unwrap().2, "a{{hex}}:::big".len());
        assert!(read_link(" <--> b").is_some_and(|l| l.0 == Head::Arrow && l.1 == Head::Arrow));
        assert!(read_link(" --o b").is_some_and(|l| l.1 == Head::Circle));
        assert!(read_link(" --- b").is_some_and(|l| l.1 == Head::None));
        assert!(read_link(" ~~~ b").is_some_and(|l| l.4));
    }

    #[test]
    fn subgraphs_are_drawn_round_their_nodes() {
        let scene = render("graph TD\nsubgraph one [First group]\n a --> b\nend\nsubgraph two\n c\nend\nb --> c").unwrap();
        let groups: Vec<&Item> = scene.items.iter().filter(|i| matches!(i, Item::Rect { fill: Paint::Group, .. })).collect();
        assert_eq!(groups.len(), 2);
        assert!(scene.words().contains(&"First group"));
        assert!(scene.words().contains(&"two"));
        // The first group's box takes in both its nodes.
        let Item::Rect { y, h, .. } = groups.iter().find(|g| matches!(g, Item::Rect { h, .. } if *h > 80.0)).unwrap() else { panic!() };
        assert!(*h > 80.0 && *y >= 0.0);
    }

    #[test]
    fn a_sequence_diagram_has_its_people_messages_notes_and_frames() {
        let source = "sequenceDiagram\n  autonumber\n  actor U as User\n  participant S as Server\n  U->>+S: Log in; please\n  S-->>-U: OK\n  Note over U,S: Talking\n  loop Every minute\n    U-)S: Ping\n  alt Fine\n    S->>S: Think\n  else Broken\n    S--xU: Error\n  end\n  end";
        let scene = render(source).unwrap();
        let words = scene.words();
        for word in ["User", "Server", "Log in; please", "OK", "Talking", "loop", "[Every minute]", "Ping", "alt", "[Fine]", "[Broken]", "Think", "Error", "1", "2"] {
            assert!(words.contains(&word), "{word} is drawn: {words:?}");
        }
        // The names appear at the top and the bottom.
        assert_eq!(words.iter().filter(|w| **w == "Server").count(), 2);
    }

    #[test]
    fn a_state_diagram_has_its_start_end_and_composite_states() {
        let source = "stateDiagram-v2\n  [*] --> Still\n  Still --> Moving : push\n  Moving --> [*]\n  state Moving {\n    [*] --> Slow\n    Slow --> Fast\n  }\n  state choose <<choice>>\n  Still --> choose\n  Still : not moving";
        let scene = render(source).unwrap();
        let words = scene.words();
        for word in ["Still", "push", "Slow", "Fast", "not moving", "Moving"] {
            assert!(words.contains(&word), "{word}: {words:?}");
        }
        let starts = scene.items.iter().filter(|i| matches!(i, Item::Ellipse { fill: Paint::Text, .. })).count();
        assert!(starts >= 3, "two starts and an end's dot");
    }

    #[test]
    fn a_class_diagram_has_its_classes_members_and_relations() {
        let source = "classDiagram\n  Animal <|-- Duck\n  Animal \"1\" *-- \"many\" Leg : has\n  class Duck {\n    +String beak\n    +swim() void\n  }\n  <<interface>> Animal\n  Animal : +int age\n  class List~T~";
        let mut found = Vec::new();
        for line in ["Animal <|-- Duck", "Animal \"1\" *-- \"many\" Leg", "A ..> B", "A --o B", "A -- B"] {
            found.push(relation(line).map(|(_, l, r, d)| (l, r, d)));
        }
        assert_eq!(relation("Animal : +int age"), None);
        assert_eq!(found[0], Some((Head::Triangle, Head::None, false)));
        assert_eq!(found[1], Some((Head::Diamond(true), Head::None, false)));
        assert_eq!(found[2], Some((Head::None, Head::Arrow, true)));
        assert_eq!(found[3], Some((Head::None, Head::Diamond(false), false)));
        assert_eq!(found[4], Some((Head::None, Head::None, false)));
        let scene = render(source).unwrap();
        let words = scene.words();
        for word in ["Animal", "Duck", "Leg", "+String beak", "+swim() void", "\u{AB}interface\u{BB}", "+int age", "has", "1", "many", "List<T>"] {
            assert!(words.contains(&word), "{word}: {words:?}");
        }
    }

    #[test]
    fn an_er_diagram_has_entities_attributes_and_crows_feet() {
        let source = "erDiagram\n  CUSTOMER ||--o{ ORDER : places\n  ORDER ||--|{ LINE-ITEM : contains\n  CUSTOMER {\n    string name PK\n    string email \"their address\"\n  }";
        let scene = render(source).unwrap();
        let words = scene.words();
        for word in ["CUSTOMER", "ORDER", "LINE-ITEM", "places", "contains", "name", "PK", "email"] {
            assert!(words.contains(&word), "{word}: {words:?}");
        }
        assert!(!words.iter().any(|w| w.contains("their address")), "comments aren't shown");
    }

    #[test]
    fn a_pie_chart_shares_its_circle() {
        let scene = render("pie showData\n  title Pets\n  \"Dogs\" : 386\n  \"Cats\" : 85.5\n  \"Rats\" : 15").unwrap();
        let words = scene.words();
        assert!(words.contains(&"Pets") && words.contains(&"Dogs [386]") && words.contains(&"Cats [85.5]"));
        assert!(words.contains(&"79%"), "{words:?}");
        let slices = scene.items.iter().filter(|i| matches!(i, Item::Path { closed: true, fill: Paint::Series(_), .. })).count();
        assert_eq!(slices, 3);
    }

    #[test]
    fn a_gantt_chart_puts_its_tasks_on_the_calendar() {
        let source = "gantt\n  title Plan\n  dateFormat YYYY-MM-DD\n  section Making\n  Design :done, a1, 2024-01-01, 10d\n  Build :crit, after a1, 2w\n  section Showing\n  Launch : milestone, 2024-01-25, 0d\n  Rest : 3d";
        let lines = statements(source);
        let scene = gantt(&lines[1..]).unwrap();
        let words = scene.words();
        for word in ["Plan", "Making", "Showing", "Design", "Build", "Launch", "Rest", "2024-01-01"] {
            assert!(words.contains(&word), "{word}: {words:?}");
        }
        assert_eq!(day_number("2024-01-11"), Some(19733.0));
        assert_eq!(duration("2w"), Some(14.0));
        // Build starts where Design ends and lasts two weeks; it's critical, so drawn in the warning colour.
        let bars: Vec<(f32, f32)> = scene.items.iter().filter_map(|i| match i { Item::Rect { x, w, fill: Paint::Warning, .. } => Some((*x, *w)), _ => None }).collect();
        let design: Vec<(f32, f32)> = scene.items.iter().filter_map(|i| match i { Item::Rect { x, w, fill: Paint::Faint, .. } => Some((*x, *w)), _ => None }).collect();
        assert_eq!(bars.len(), 1);
        assert!((design[0].0 + design[0].1 - bars[0].0).abs() < 0.01);
        assert!((bars[0].1 / design[0].1 - 1.4).abs() < 0.01);
        assert!(render("gantt\n  dateFormat DD/MM/YYYY\n  A task : 01/02/2024, 3d").is_err(), "a date it can't read is said");
    }

    #[test]
    fn other_kinds_and_nonsense_are_turned_down_gently() {
        assert!(render("").is_err());
        assert!(render("journey\n title My day").unwrap_err().contains("journey"));
        assert!(render("flowchart TD").is_err(), "nothing to draw");
        for nonsense in ["graph", "graph TD\n-->", "graph TD\nA-->", "graph TD\n[[[", "sequenceDiagram\n->>", "sequenceDiagram\nend\nend", "classDiagram\n<|--", "stateDiagram\n}\n}", "erDiagram\n{", "pie\n:", "gantt\n:", "graph TD\nA[\"unclosed"] {
            let _ = render(nonsense);
        }
        assert_eq!(boxes(&render("graph TD\nA-->B").unwrap()), 2);
    }
}
