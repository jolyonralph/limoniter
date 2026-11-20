//! Diagrams, drawn the same way on the screen and in a PDF: a `Scene` of boxes, lines and words in points, with
//! colours given by what they're for rather than what they are, so they suit the theme they're drawn in. And the
//! layout for anything drawn as boxes joined by arrows (Mermaid's flowcharts, state and class diagrams, and an
//! assembly program's calls): layered, as Sugiyama described, with each box on the layer below the longest
//! path into it, the order in each layer chosen to cross as few lines as it can (each box put at the average place
//! of the boxes it's joined to, over and over), and the boxes then spread out so each sits as near its neighbours
//! as the room allows.

use eframe::egui::{self, Color32, Stroke};

/// What a colour is for. The screen takes it from the theme; a PDF from a palette for paper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    None,
    Text,
    /// Words that matter less, such as a lifeline's or an axis's.
    Faint,
    Line,
    /// A box's inside.
    Fill,
    /// A group's inside, behind its boxes.
    Group,
    Note,
    Accent,
    /// The nth colour of a set, for a pie's slices or a chart's bars.
    Series(u8),
    /// Something marked as urgent (a Gantt chart's critical tasks).
    Warning,
    /// The page behind, to clear a space for words over a line.
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Middle,
}

/// One thing drawn. Places are in points from the scene's top left.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A box, its corners rounded by `radius`.
    Rect { x: f32, y: f32, w: f32, h: f32, radius: f32, fill: Paint, stroke: Paint, dashed: bool },
    Ellipse { cx: f32, cy: f32, rx: f32, ry: f32, fill: Paint, stroke: Paint },
    /// A line through points, or a shape if it's closed.
    Path { points: Vec<(f32, f32)>, closed: bool, fill: Paint, stroke: Paint, width: f32, dashed: bool },
    /// A line of words: `y` is the middle of the line, and `x` its start, middle or end.
    Text { x: f32, y: f32, text: String, size: f32, align: Align, bold: bool, paint: Paint },
}

/// A diagram ready to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub width: f32,
    pub height: f32,
    pub items: Vec<Item>,
}

/// How wide words are at a size: Helvetica's widths, which the PDF uses and the screen's font is near enough.
pub fn text_width(text: &str, size: f32, bold: bool) -> f32 {
    crate::pdf::text_width(text, if bold { crate::pdf::Font::Bold } else { crate::pdf::Font::Regular }, size)
}

impl Scene {
    pub fn text(&mut self, x: f32, y: f32, text: impl Into<String>, size: f32, align: Align, bold: bool, paint: Paint) {
        self.items.push(Item::Text { x, y, text: text.into(), size, align, bold, paint });
    }

    /// Lines of words centred on a point, one under another.
    pub fn lines(&mut self, x: f32, y: f32, lines: &[String], size: f32, bold: bool, paint: Paint) {
        let height = size * LINE;
        let top = y - height * (lines.len() as f32 - 1.0) / 2.0;
        for (n, line) in lines.iter().enumerate() {
            self.text(x, top + n as f32 * height, line.clone(), size, Align::Middle, bold, paint);
        }
    }

    pub fn line(&mut self, points: Vec<(f32, f32)>, paint: Paint, width: f32, dashed: bool) {
        self.items.push(Item::Path { points, closed: false, fill: Paint::None, stroke: paint, width, dashed });
    }

    pub fn polygon(&mut self, points: Vec<(f32, f32)>, fill: Paint, stroke: Paint) {
        self.items.push(Item::Path { points, closed: true, fill, stroke, width: 1.0, dashed: false });
    }

    /// Grows the scene to take in everything in it, with a margin round it.
    pub fn fit(&mut self, margin: f32) {
        let (mut left, mut top, mut right, mut bottom) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let mut take = |x: f32, y: f32| {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        };
        for item in &self.items {
            match item {
                Item::Rect { x, y, w, h, .. } => {
                    take(*x, *y);
                    take(x + w, y + h);
                }
                Item::Ellipse { cx, cy, rx, ry, .. } => {
                    take(cx - rx, cy - ry);
                    take(cx + rx, cy + ry);
                }
                Item::Path { points, .. } => points.iter().for_each(|(x, y)| take(*x, *y)),
                Item::Text { x, y, text, size, align, bold, .. } => {
                    let width = text_width(text, *size, *bold);
                    let start = match align {
                        Align::Start => *x,
                        Align::Middle => x - width / 2.0,
                    };
                    take(start, y - size * 0.6);
                    take(start + width, y + size * 0.6);
                }
            }
        }
        if left > right {
            self.width = margin * 2.0;
            self.height = margin * 2.0;
            return;
        }
        let (dx, dy) = (margin - left, margin - top);
        self.shift(dx, dy);
        self.width = right - left + margin * 2.0;
        self.height = bottom - top + margin * 2.0;
    }

    pub fn shift(&mut self, dx: f32, dy: f32) {
        for item in &mut self.items {
            match item {
                Item::Rect { x, y, .. } => {
                    *x += dx;
                    *y += dy;
                }
                Item::Ellipse { cx, cy, .. } => {
                    *cx += dx;
                    *cy += dy;
                }
                Item::Path { points, .. } => points.iter_mut().for_each(|(x, y)| {
                    *x += dx;
                    *y += dy;
                }),
                Item::Text { x, y, .. } => {
                    *x += dx;
                    *y += dy;
                }
            }
        }
    }

    /// All the words in it, for tests.
    #[cfg(test)]
    pub fn words(&self) -> Vec<&str> {
        self.items
            .iter()
            .filter_map(|item| match item {
                Item::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// How tall a line of words is, for its size.
pub const LINE: f32 = 1.3;

// ---------- Shapes ----------

/// The shapes a box can be, as Mermaid's flowcharts have them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Round,
    Stadium,
    Subroutine,
    Cylinder,
    Circle,
    DoubleCircle,
    Diamond,
    Hexagon,
    /// Leaning right, or left.
    Lean(bool),
    /// Wider at the bottom, or at the top.
    Trapezoid(bool),
    /// A flag shape, pointed on the left (Mermaid's `>text]`).
    Flag,
    /// A start state: a filled dot.
    Start,
    /// An end state: a dot in a ring.
    End,
    /// A fork or join: a thick bar.
    Bar,
}

/// Draws a shape in a box.
pub fn shape(scene: &mut Scene, shape: Shape, x: f32, y: f32, w: f32, h: f32, fill: Paint, stroke: Paint) {
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    match shape {
        Shape::Rect => scene.items.push(Item::Rect { x, y, w, h, radius: 0.0, fill, stroke, dashed: false }),
        Shape::Round => scene.items.push(Item::Rect { x, y, w, h, radius: 5.0, fill, stroke, dashed: false }),
        Shape::Stadium => scene.items.push(Item::Rect { x, y, w, h, radius: h / 2.0, fill, stroke, dashed: false }),
        Shape::Subroutine => {
            scene.items.push(Item::Rect { x, y, w, h, radius: 0.0, fill, stroke, dashed: false });
            scene.line(vec![(x + 7.0, y), (x + 7.0, y + h)], stroke, 1.0, false);
            scene.line(vec![(x + w - 7.0, y), (x + w - 7.0, y + h)], stroke, 1.0, false);
        }
        Shape::Cylinder => {
            let ry = (h * 0.12).min(8.0);
            let mut outline = arc(cx, y + ry, w / 2.0, ry, 180.0, 360.0);
            outline.extend(arc(cx, y + h - ry, w / 2.0, ry, 0.0, 180.0));
            scene.polygon(outline, fill, stroke);
            scene.line(arc(cx, y + ry, w / 2.0, ry, 0.0, 180.0), stroke, 1.0, false);
        }
        Shape::Circle => scene.items.push(Item::Ellipse { cx, cy, rx: w / 2.0, ry: h / 2.0, fill, stroke }),
        Shape::DoubleCircle => {
            scene.items.push(Item::Ellipse { cx, cy, rx: w / 2.0, ry: h / 2.0, fill, stroke });
            scene.items.push(Item::Ellipse { cx, cy, rx: w / 2.0 - 4.0, ry: h / 2.0 - 4.0, fill: Paint::None, stroke });
        }
        Shape::Diamond => scene.polygon(vec![(cx, y), (x + w, cy), (cx, y + h), (x, cy)], fill, stroke),
        Shape::Hexagon => {
            let cut = (h / 2.0).min(w / 4.0);
            scene.polygon(vec![(x + cut, y), (x + w - cut, y), (x + w, cy), (x + w - cut, y + h), (x + cut, y + h), (x, cy)], fill, stroke);
        }
        Shape::Lean(right) => {
            let s = h * 0.4;
            let points = if right { vec![(x + s, y), (x + w, y), (x + w - s, y + h), (x, y + h)] } else { vec![(x, y), (x + w - s, y), (x + w, y + h), (x + s, y + h)] };
            scene.polygon(points, fill, stroke);
        }
        Shape::Trapezoid(wide_bottom) => {
            let s = h * 0.4;
            let points = if wide_bottom { vec![(x + s, y), (x + w - s, y), (x + w, y + h), (x, y + h)] } else { vec![(x, y), (x + w, y), (x + w - s, y + h), (x + s, y + h)] };
            scene.polygon(points, fill, stroke);
        }
        // Starting at the notch, so the screen's fan of triangles from the first point fills it rightly.
        Shape::Flag => scene.polygon(vec![(x + h * 0.35, cy), (x, y), (x + w, y), (x + w, y + h), (x, y + h)], fill, stroke),
        Shape::Start => scene.items.push(Item::Ellipse { cx, cy, rx: w / 2.0, ry: h / 2.0, fill: Paint::Text, stroke: Paint::Text }),
        Shape::End => {
            scene.items.push(Item::Ellipse { cx, cy, rx: w / 2.0, ry: h / 2.0, fill: Paint::None, stroke: Paint::Text });
            scene.items.push(Item::Ellipse { cx, cy, rx: w / 2.0 - 3.5, ry: h / 2.0 - 3.5, fill: Paint::Text, stroke: Paint::Text });
        }
        Shape::Bar => scene.items.push(Item::Rect { x, y, w, h, radius: 0.0, fill: Paint::Text, stroke: Paint::Text, dashed: false }),
    }
}

/// Points round part of an ellipse, by degrees (0 is to the right, going clockwise down the page).
pub fn arc(cx: f32, cy: f32, rx: f32, ry: f32, from: f32, to: f32) -> Vec<(f32, f32)> {
    let steps = (((to - from).abs() / 6.0).ceil() as usize).max(2);
    (0..=steps)
        .map(|n| {
            let angle = (from + (to - from) * n as f32 / steps as f32).to_radians();
            (cx + rx * angle.cos(), cy + ry * angle.sin())
        })
        .collect()
}

/// Where a line from a shape's middle towards a point leaves its edge.
pub fn edge_point(shape: Shape, cx: f32, cy: f32, w: f32, h: f32, toward: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (toward.0 - cx, toward.1 - cy);
    if dx.abs() < 0.001 && dy.abs() < 0.001 {
        return (cx, cy);
    }
    let (hw, hh) = (w / 2.0, h / 2.0);
    let t = match shape {
        Shape::Circle | Shape::DoubleCircle | Shape::Start | Shape::End => 1.0 / ((dx / hw).powi(2) + (dy / hh).powi(2)).sqrt(),
        Shape::Diamond => 1.0 / (dx.abs() / hw + dy.abs() / hh),
        _ => (hw / dx.abs()).min(hh / dy.abs()),
    };
    (cx + dx * t, cy + dy * t)
}

/// The ends a line can have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Head {
    None,
    Arrow,
    /// An open arrow: two lines, as a sequence diagram's asynchronous messages have.
    Open,
    Cross,
    Circle,
    /// A hollow triangle: a class inheriting.
    Triangle,
    /// A filled diamond (composition), or a hollow one (aggregation).
    Diamond(bool),
}

/// Draws a line's end at its last point, pointing the way its last part goes.
pub fn head(scene: &mut Scene, head: Head, tip: (f32, f32), from: (f32, f32), paint: Paint) {
    let (dx, dy) = (tip.0 - from.0, tip.1 - from.1);
    let length = (dx * dx + dy * dy).sqrt().max(0.001);
    let (ux, uy) = (dx / length, dy / length);
    let (px, py) = (-uy, ux);
    let at = |back: f32, side: f32| (tip.0 - ux * back + px * side, tip.1 - uy * back + py * side);
    match head {
        Head::None => {}
        Head::Arrow => scene.polygon(vec![tip, at(9.0, 4.5), at(9.0, -4.5)], paint, paint),
        Head::Open => {
            scene.line(vec![at(9.0, 4.5), tip, at(9.0, -4.5)], paint, 1.2, false);
        }
        Head::Cross => {
            scene.line(vec![at(10.0, 4.5), at(1.0, -4.5)], paint, 1.5, false);
            scene.line(vec![at(10.0, -4.5), at(1.0, 4.5)], paint, 1.5, false);
        }
        Head::Circle => {
            let (cx, cy) = at(4.0, 0.0);
            scene.items.push(Item::Ellipse { cx, cy, rx: 4.0, ry: 4.0, fill: Paint::Fill, stroke: paint });
        }
        Head::Triangle => scene.polygon(vec![tip, at(12.0, 6.5), at(12.0, -6.5)], Paint::Fill, paint),
        Head::Diamond(filled) => scene.polygon(vec![tip, at(8.0, 5.0), at(16.0, 0.0), at(8.0, -5.0)], if filled { paint } else { Paint::Fill }, paint),
    }
}

/// A line with rounded corners: each corner made into a curve between the middles of its two sides.
pub fn smooth(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut out = vec![points[0]];
    for n in 1..points.len() - 1 {
        let (a, b, c) = (points[n - 1], points[n], points[n + 1]);
        let start = if n == 1 { a } else { ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0) };
        let end = if n == points.len() - 2 { c } else { ((b.0 + c.0) / 2.0, (b.1 + c.1) / 2.0) };
        for step in 1..=8 {
            let t = step as f32 / 8.0;
            let u = 1.0 - t;
            out.push((u * u * start.0 + 2.0 * u * t * b.0 + t * t * end.0, u * u * start.1 + 2.0 * u * t * b.1 + t * t * end.1));
        }
        let _ = start;
    }
    if out.last() != points.last() {
        out.push(points[points.len() - 1]);
    }
    out
}

// ---------- Layered layout ----------

/// Which way the layers go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Down,
    Up,
    Right,
    Left,
}

impl Direction {
    /// Mermaid's way of writing it: TD, TB, BT, LR, RL.
    pub fn from_mermaid(text: &str) -> Option<Direction> {
        match text.to_ascii_uppercase().as_str() {
            "TD" | "TB" | "V" => Some(Direction::Down),
            "BT" | "^" => Some(Direction::Up),
            "LR" | ">" => Some(Direction::Right),
            "RL" | "<" => Some(Direction::Left),
            _ => None,
        }
    }

    fn across(self) -> bool {
        matches!(self, Direction::Right | Direction::Left)
    }
}

/// A box to place: its size, and the group it's in.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub width: f32,
    pub height: f32,
    pub group: Option<usize>,
}

/// A line between two boxes, and the size of its words, if it has any.
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: Option<(f32, f32)>,
}

/// Where everything went.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    /// Each box's middle.
    pub centres: Vec<(f32, f32)>,
    /// Each line's points, from the middle of the box it leaves to the middle of the one it reaches; a line from a box
    /// to itself goes round its corner.
    pub routes: Vec<Vec<(f32, f32)>>,
    /// Where each line's words go.
    pub labels: Vec<Option<(f32, f32)>>,
}

/// A box in the layered graph: one of the real ones, or a stand-in that a long line passes through.
#[derive(Clone, Debug)]
struct Slot {
    /// Its size along the layer and across it.
    along: f32,
    across: f32,
    group: Option<usize>,
    rank: usize,
}

/// Spaces between boxes in a layer, and between layers.
pub struct Gaps {
    pub node: f32,
    pub rank: f32,
}

impl Default for Gaps {
    fn default() -> Self {
        Gaps { node: 30.0, rank: 50.0 }
    }
}

/// Lays boxes out in layers. Lines that would go back up are turned round while it's done, so loops work.
pub fn layered(nodes: &[Node], edges: &[Edge], direction: Direction, gaps: &Gaps) -> Layout {
    let count = nodes.len();
    let mut layout = Layout { centres: vec![(0.0, 0.0); count], routes: vec![Vec::new(); edges.len()], labels: vec![None; edges.len()] };
    if count == 0 {
        return layout;
    }
    // The lines that go somewhere else, with those that would close a loop turned round.
    let reversed = back_edges(count, edges);
    let forward: Vec<(usize, usize)> = edges.iter().enumerate().filter(|(_, e)| e.from != e.to && e.from < count && e.to < count).map(|(n, e)| if reversed[n] { (e.to, e.from) } else { (e.from, e.to) }).collect();
    let ranks = ranks(count, &forward);

    // The slots: the real boxes, then stand-ins along each line that crosses more than one layer.
    let mut slots: Vec<Slot> = nodes
        .iter()
        .zip(&ranks)
        .map(|(node, rank)| {
            let (along, across) = if direction.across() { (node.height, node.width) } else { (node.width, node.height) };
            Slot { along, across, group: node.group, rank: *rank }
        })
        .collect();
    // Each line's chain of slots, and the slot its words sit in.
    let mut chains: Vec<Vec<usize>> = vec![Vec::new(); edges.len()];
    let mut label_slots: Vec<Option<usize>> = vec![None; edges.len()];
    for (n, edge) in edges.iter().enumerate() {
        if edge.from == edge.to || edge.from >= count || edge.to >= count {
            continue;
        }
        let (a, b) = if reversed[n] { (edge.to, edge.from) } else { (edge.from, edge.to) };
        let group = if nodes[a].group == nodes[b].group { nodes[a].group } else { None };
        let mut chain = vec![a];
        for rank in ranks[a] + 1..ranks[b] {
            let label = (rank == ranks[a] + 1).then_some(edge.label).flatten();
            let (along, across) = match label {
                Some((w, h)) if direction.across() => (h, w),
                Some((w, h)) => (w, h),
                None => (0.0, 0.0),
            };
            if label.is_some() {
                label_slots[n] = Some(slots.len());
            }
            chain.push(slots.len());
            slots.push(Slot { along, across, group, rank });
        }
        chain.push(b);
        chains[n] = chain;
    }
    let deepest = slots.iter().map(|s| s.rank).max().unwrap_or(0);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); deepest + 1];
    // The first order: the boxes as they were given, which is usually the order they were written in.
    for (n, slot) in slots.iter().enumerate() {
        layers[slot.rank].push(n);
    }
    let mut ups: Vec<Vec<usize>> = vec![Vec::new(); slots.len()];
    let mut downs: Vec<Vec<usize>> = vec![Vec::new(); slots.len()];
    for chain in &chains {
        for pair in chain.windows(2) {
            downs[pair[0]].push(pair[1]);
            ups[pair[1]].push(pair[0]);
        }
    }
    order_layers(&mut layers, &slots, &ups, &downs);
    let along = place_along(&layers, &slots, &ups, &downs, gaps.node);

    // Across the layers: each as deep as its deepest box, with half a gap either side of the layers between.
    let mut depth = vec![0.0_f32; layers.len()];
    for slot in &slots {
        depth[slot.rank] = depth[slot.rank].max(slot.across);
    }
    let mut across = vec![0.0_f32; layers.len()];
    let mut at = 0.0;
    for rank in 0..layers.len() {
        if rank > 0 {
            at += depth[rank - 1] / 2.0 + gaps.rank / 2.0 + depth[rank] / 2.0;
        }
        across[rank] = at;
    }
    let total = at;
    let point = |slot: usize| -> (f32, f32) {
        let (u, v) = (along[slot], across[slots[slot].rank]);
        match direction {
            Direction::Down => (u, v),
            Direction::Up => (u, total - v),
            Direction::Right => (v, u),
            Direction::Left => (total - v, u),
        }
    };
    for (n, centre) in layout.centres.iter_mut().enumerate() {
        *centre = point(n);
    }
    for (n, chain) in chains.iter().enumerate() {
        let mut points: Vec<(f32, f32)> = chain.iter().map(|slot| point(*slot)).collect();
        if reversed[n] {
            points.reverse();
        }
        layout.routes[n] = points;
        layout.labels[n] = label_slots[n].map(point);
    }
    // A line from a box back to itself: round its corner.
    for (n, edge) in edges.iter().enumerate() {
        if edge.from == edge.to && edge.from < count {
            let (cx, cy) = layout.centres[edge.from];
            let (w, h) = (nodes[edge.from].width, nodes[edge.from].height);
            let (x, y) = (cx + w / 2.0, cy - h / 2.0);
            layout.routes[n] = vec![(x - w * 0.2, y), (x - w * 0.2, y - 18.0), (x + 18.0, y - 18.0), (x + 18.0, y + h * 0.3), (x, y + h * 0.3)];
            layout.labels[n] = edge.label.map(|(lw, _)| (x + 22.0 + lw / 2.0, y - 18.0));
        }
    }
    layout
}

/// The lines that close a loop, found by going depth first from each box in turn.
fn back_edges(count: usize, edges: &[Edge]) -> Vec<bool> {
    let mut out = vec![false; edges.len()];
    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (n, edge) in edges.iter().enumerate() {
        if edge.from < count && edge.to < count && edge.from != edge.to {
            outgoing[edge.from].push(n);
        }
    }
    // 0 not seen, 1 on the way down, 2 done.
    let mut state = vec![0u8; count];
    for start in 0..count {
        if state[start] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        state[start] = 1;
        while let Some((node, next)) = stack.last_mut() {
            if let Some(&edge) = outgoing[*node].get(*next) {
                *next += 1;
                let to = edges[edge].to;
                match state[to] {
                    0 => {
                        state[to] = 1;
                        stack.push((to, 0));
                    }
                    1 => out[edge] = true,
                    _ => {}
                }
            } else {
                state[*node] = 2;
                stack.pop();
            }
        }
    }
    out
}

/// Each box's layer: two below the deepest box leading to it, so there's a layer between for the lines' words.
/// A box nothing leads to is moved down to just above the first box it leads to.
fn ranks(count: usize, forward: &[(usize, usize)]) -> Vec<usize> {
    let mut incoming = vec![0usize; count];
    for (_, to) in forward {
        incoming[*to] += 1;
    }
    let mut ready: Vec<usize> = (0..count).filter(|n| incoming[*n] == 0).rev().collect();
    let mut order = Vec::with_capacity(count);
    let mut left = incoming.clone();
    while let Some(node) = ready.pop() {
        order.push(node);
        for (_, to) in forward.iter().filter(|(from, _)| *from == node) {
            left[*to] -= 1;
            if left[*to] == 0 {
                ready.push(*to);
            }
        }
    }
    let mut rank = vec![0usize; count];
    for &node in &order {
        for (_, to) in forward.iter().filter(|(from, _)| *from == node) {
            rank[*to] = rank[*to].max(rank[node] + 2);
        }
    }
    for &node in order.iter().rev() {
        if incoming[node] == 0
            && let Some(lowest) = forward.iter().filter(|(from, _)| *from == node).map(|(_, to)| rank[*to]).min()
        {
            rank[node] = rank[node].max(lowest.saturating_sub(2));
        }
    }
    rank
}

/// How many lines cross between two layers.
fn crossings(upper: &[usize], lower: &[usize], downs: &[Vec<usize>]) -> usize {
    let mut place = std::collections::HashMap::new();
    for (n, slot) in lower.iter().enumerate() {
        place.insert(*slot, n);
    }
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (n, slot) in upper.iter().enumerate() {
        for to in &downs[*slot] {
            if let Some(at) = place.get(to) {
                pairs.push((n, *at));
            }
        }
    }
    let mut count = 0;
    for (i, a) in pairs.iter().enumerate() {
        for b in &pairs[i + 1..] {
            if (a.0 < b.0 && a.1 > b.1) || (a.0 > b.0 && a.1 < b.1) {
                count += 1;
            }
        }
    }
    count
}

fn all_crossings(layers: &[Vec<usize>], downs: &[Vec<usize>]) -> usize {
    layers.windows(2).map(|pair| crossings(&pair[0], &pair[1], downs)).sum()
}

/// Orders each layer to cross as few lines as it can: down and up again, each box put at the average place of
/// those it's joined to in the layer before, keeping a group's boxes side by side. The best order found is kept.
fn order_layers(layers: &mut [Vec<usize>], slots: &[Slot], ups: &[Vec<usize>], downs: &[Vec<usize>]) {
    for layer in layers.iter_mut() {
        group_together(layer, slots);
    }
    let mut best = layers.to_vec();
    let mut best_count = all_crossings(layers, downs);
    for round in 0..16 {
        let going_down = round % 2 == 0;
        let range: Vec<usize> = if going_down { (1..layers.len()).collect() } else { (0..layers.len().saturating_sub(1)).rev().collect() };
        for rank in range {
            let (neighbours, other) = if going_down { (ups, rank - 1) } else { (downs, rank + 1) };
            let place: std::collections::HashMap<usize, f32> = layers[other].iter().enumerate().map(|(n, s)| (*s, n as f32)).collect();
            let current: std::collections::HashMap<usize, f32> = layers[rank].iter().enumerate().map(|(n, s)| (*s, n as f32)).collect();
            let centre = |slot: usize| -> f32 {
                let found: Vec<f32> = neighbours[slot].iter().filter_map(|n| place.get(n)).copied().collect();
                if found.is_empty() { current[&slot] * (layers[other].len().max(1) as f32 / layers[rank].len().max(1) as f32) } else { found.iter().sum::<f32>() / found.len() as f32 }
            };
            let centres: std::collections::HashMap<usize, f32> = layers[rank].iter().map(|s| (*s, centre(*s))).collect();
            // A group goes where its boxes would on average.
            let mut group_centre: std::collections::HashMap<usize, (f32, usize)> = std::collections::HashMap::new();
            for slot in &layers[rank] {
                if let Some(group) = slots[*slot].group {
                    let entry = group_centre.entry(group).or_insert((0.0, 0));
                    entry.0 += centres[slot];
                    entry.1 += 1;
                }
            }
            layers[rank].sort_by(|a, b| {
                let key = |slot: &usize| {
                    let own = centres[slot];
                    match slots[*slot].group {
                        Some(group) => {
                            let (sum, n) = group_centre[&group];
                            (sum / n as f32, group, own)
                        }
                        None => (own, usize::MAX, own),
                    }
                };
                let (ka, kb) = (key(a), key(b));
                ka.0.total_cmp(&kb.0).then(ka.1.cmp(&kb.1)).then(ka.2.total_cmp(&kb.2))
            });
        }
        let count = all_crossings(layers, downs);
        if count < best_count {
            best_count = count;
            best = layers.to_vec();
        }
        if best_count == 0 {
            break;
        }
    }
    layers.clone_from_slice(&best);
}

/// Moves each group's boxes in a layer up beside the first of them, keeping the order otherwise.
fn group_together(layer: &mut [usize], slots: &[Slot]) {
    let mut first: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    let keys: Vec<(usize, usize)> = layer
        .iter()
        .enumerate()
        .map(|(n, slot)| match slots[*slot].group {
            Some(group) => (*first.entry(group).or_insert(n), n),
            None => (n, n),
        })
        .collect();
    let mut order: Vec<usize> = (0..layer.len()).collect();
    order.sort_by_key(|n| keys[*n]);
    let moved: Vec<usize> = order.iter().map(|n| layer[*n]).collect();
    layer.copy_from_slice(&moved);
}

/// Places the boxes along each layer, in their order, at least `gap` apart: each as near the average of those
/// it's joined to as it can be. A run of boxes pushed together moves as one, to the average of where each would
/// like to be, which is the best they can do in a line.
fn place_along(layers: &[Vec<usize>], slots: &[Slot], ups: &[Vec<usize>], downs: &[Vec<usize>], gap: f32) -> Vec<f32> {
    let mut at = vec![0.0_f32; slots.len()];
    let space = |a: usize, b: usize| -> f32 {
        let between = if slots[a].along == 0.0 || slots[b].along == 0.0 { gap / 2.0 } else { gap };
        let between = if slots[a].group != slots[b].group { between + 16.0 } else { between };
        slots[a].along / 2.0 + between + slots[b].along / 2.0
    };
    for layer in layers {
        let mut x = 0.0;
        for (n, slot) in layer.iter().enumerate() {
            if n > 0 {
                x += space(layer[n - 1], *slot);
            }
            at[*slot] = x;
        }
    }
    for round in 0..12 {
        let order: Vec<usize> = if round % 2 == 0 { (0..layers.len()).collect() } else { (0..layers.len()).rev().collect() };
        for rank in order {
            let layer = &layers[rank];
            if layer.is_empty() {
                continue;
            }
            let wanted: Vec<f32> = layer
                .iter()
                .map(|slot| {
                    let joined: Vec<f32> = match round {
                        r if r < 4 && r % 2 == 0 => ups[*slot].iter().map(|n| at[*n]).collect(),
                        r if r < 4 => downs[*slot].iter().map(|n| at[*n]).collect(),
                        _ => ups[*slot].iter().chain(&downs[*slot]).map(|n| at[*n]).collect(),
                    };
                    if joined.is_empty() { at[*slot] } else { joined.iter().sum::<f32>() / joined.len() as f32 }
                })
                .collect();
            // The least each box must be from the first.
            let mut offsets = vec![0.0_f32; layer.len()];
            for n in 1..layer.len() {
                offsets[n] = offsets[n - 1] + space(layer[n - 1], layer[n]);
            }
            // Runs pushed together: (first, last, their base).
            let mut runs: Vec<(usize, usize, f32)> = Vec::new();
            for n in 0..layer.len() {
                runs.push((n, n, wanted[n] - offsets[n]));
                while runs.len() > 1 {
                    let last = runs[runs.len() - 1];
                    let before = runs[runs.len() - 2];
                    // Each run's boxes share a base, each box its own distance from it; runs are apart enough
                    // while each base is no further left than the one before.
                    if before.2 <= last.2 {
                        break;
                    }
                    // Joined: the base that's best for them all is the average of what each asks of it.
                    let (first, end) = (before.0, last.1);
                    let mean = (first..=end).map(|k| wanted[k] - offsets[k]).sum::<f32>() / (end - first + 1) as f32;
                    runs.pop();
                    runs.pop();
                    runs.push((first, end, mean));
                }
            }
            for (first, end, start) in runs {
                for k in first..=end {
                    at[layer[k]] = start + offsets[k];
                }
            }
        }
    }
    // Everything from the left edge, with room for the widest box's half.
    let least = slots.iter().enumerate().map(|(n, s)| at[n] - s.along / 2.0).fold(f32::MAX, f32::min);
    if least.is_finite() {
        at.iter_mut().for_each(|x| *x -= least);
    }
    at
}

// ---------- Drawing on the screen ----------

/// A colour for the screen, from the theme.
pub fn screen_colour(paint: Paint) -> Color32 {
    use crate::theme;
    match paint {
        Paint::None => Color32::TRANSPARENT,
        Paint::Text => theme::text(),
        Paint::Faint => theme::text_dim(),
        Paint::Line => theme::mix(theme::text(), theme::field(), 0.25),
        Paint::Fill => theme::mix(theme::accent(), theme::field(), 0.85),
        Paint::Group => theme::mix(theme::text(), theme::field(), 0.94),
        Paint::Note => theme::mix(theme::tag_3(), theme::field(), 0.7),
        Paint::Accent => theme::accent(),
        Paint::Series(n) => series(n),
        Paint::Warning => theme::tag_1(),
        Paint::Back => theme::field(),
    }
}

fn series(n: u8) -> Color32 {
    use crate::theme;
    match n % 8 {
        0 => theme::tag_5(),
        1 => theme::tag_2(),
        2 => theme::tag_4(),
        3 => theme::tag_6(),
        4 => theme::tag_1(),
        5 => theme::tag_3(),
        6 => theme::mix(theme::tag_5(), theme::field(), 0.5),
        _ => theme::mix(theme::tag_2(), theme::field(), 0.5),
    }
}

/// A colour for paper.
pub fn paper_colour(paint: Paint) -> Option<crate::pdf::Rgb> {
    use crate::pdf::Rgb;
    Some(match paint {
        Paint::None => return None,
        Paint::Text => Rgb::from_bytes(0x20, 0x20, 0x20),
        Paint::Faint => Rgb::from_bytes(0x60, 0x60, 0x60),
        Paint::Line => Rgb::from_bytes(0x50, 0x50, 0x58),
        Paint::Fill => Rgb::from_bytes(0xEC, 0xEE, 0xFB),
        Paint::Group => Rgb::from_bytes(0xF6, 0xF6, 0xF0),
        Paint::Note => Rgb::from_bytes(0xFF, 0xF5, 0xC4),
        Paint::Accent => Rgb::from_bytes(0x33, 0x55, 0xBB),
        Paint::Warning => Rgb::from_bytes(0xD0, 0x40, 0x40),
        Paint::Back => Rgb(1.0, 1.0, 1.0),
        Paint::Series(n) => {
            const PALETTE: [(u8, u8, u8); 8] = [(0x4E, 0x79, 0xA7), (0xF2, 0x8E, 0x2B), (0x59, 0xA1, 0x4F), (0xB0, 0x7A, 0xA1), (0xE1, 0x57, 0x59), (0xED, 0xC9, 0x48), (0x76, 0xB7, 0xB2), (0xFF, 0x9D, 0xA7)];
            let (r, g, b) = PALETTE[usize::from(n) % 8];
            Rgb::from_bytes(r, g, b)
        }
    })
}

/// Draws a scene with its top left at `origin`, scaled.
pub fn paint(painter: &egui::Painter, scene: &Scene, origin: egui::Pos2, scale: f32) {
    let to = |x: f32, y: f32| origin + egui::vec2(x * scale, y * scale);
    for item in &scene.items {
        match item {
            Item::Rect { x, y, w, h, radius, fill, stroke, dashed } => {
                let rect = egui::Rect::from_min_size(to(*x, *y), egui::vec2(w * scale, h * scale));
                painter.rect_filled(rect, radius * scale, screen_colour(*fill));
                if *stroke != Paint::None {
                    if *dashed {
                        let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
                        painter.extend(egui::Shape::dashed_line(&corners, Stroke::new(1.0, screen_colour(*stroke)), 5.0, 3.0));
                    } else {
                        painter.rect_stroke(rect, radius * scale, Stroke::new(1.0, screen_colour(*stroke)), egui::StrokeKind::Middle);
                    }
                }
            }
            Item::Ellipse { cx, cy, rx, ry, fill, stroke } => {
                let shape = egui::epaint::EllipseShape { center: to(*cx, *cy), radius: egui::vec2(rx * scale, ry * scale), angle: 0.0, fill: screen_colour(*fill), stroke: Stroke::new(if *stroke == Paint::None { 0.0 } else { 1.0 }, screen_colour(*stroke)) };
                painter.add(shape);
            }
            Item::Path { points, closed, fill, stroke, width, dashed } => {
                let points: Vec<egui::Pos2> = points.iter().map(|(x, y)| to(*x, *y)).collect();
                let line = Stroke::new(if *stroke == Paint::None { 0.0 } else { width * scale.max(0.6) }, screen_colour(*stroke));
                if *closed {
                    // Filled as a convex shape where it is one; a few, such as a cylinder, are near enough.
                    painter.add(egui::Shape::convex_polygon(points, screen_colour(*fill), line));
                } else if *dashed {
                    painter.extend(egui::Shape::dashed_line(&points, line, 5.0, 3.0));
                } else {
                    painter.add(egui::Shape::line(points, line));
                }
            }
            Item::Text { x, y, text, size, align, bold, paint } => {
                let anchor = match align {
                    Align::Start => egui::Align2::LEFT_CENTER,
                    Align::Middle => egui::Align2::CENTER_CENTER,
                };
                let colour = screen_colour(*paint);
                let font = egui::FontId::proportional(size * scale);
                painter.text(to(*x, *y), anchor, text, font.clone(), colour);
                if *bold {
                    // The fonts that come with the app have no bold, so it's drawn twice, a hair apart.
                    painter.text(to(*x, *y) + egui::vec2(0.5 * scale.max(0.6), 0.0), anchor, text, font, colour);
                }
            }
        }
    }
}

/// Draws a scene as part of a page of a PDF, with its top left at a place and scaled.
pub fn paint_pdf(page: &mut crate::pdf::Page, document: &crate::pdf::Document, scene: &Scene, left: f32, top: f32, scale: f32) {
    use crate::pdf::{Font, Paint as PdfPaint};
    let to = |x: f32, y: f32| (left + x * scale, top + y * scale);
    let paint_of = |fill: Paint, stroke: Paint, width: f32, dashed: bool| PdfPaint { fill: paper_colour(fill), stroke: paper_colour(stroke), width: width * scale, dashed };
    for item in &scene.items {
        match item {
            Item::Rect { x, y, w, h, radius, fill, stroke, dashed } => {
                let (px, py) = to(*x, *y);
                page.rect(document, px, py, w * scale, h * scale, radius * scale, paint_of(*fill, *stroke, 0.8, *dashed));
            }
            Item::Ellipse { cx, cy, rx, ry, fill, stroke } => {
                let (px, py) = to(*cx, *cy);
                page.ellipse(document, px, py, rx * scale, ry * scale, paint_of(*fill, *stroke, 0.8, false));
            }
            Item::Path { points, closed, fill, stroke, width, dashed } => {
                let points: Vec<(f32, f32)> = points.iter().map(|(x, y)| to(*x, *y)).collect();
                page.path(document, &points, *closed, paint_of(*fill, *stroke, width * 0.8, *dashed));
            }
            Item::Text { x, y, text, size, align, bold, paint } => {
                let font = if *bold { Font::Bold } else { Font::Regular };
                let size = size * scale;
                let width = crate::pdf::text_width(text, font, size);
                let (px, py) = to(*x, *y);
                let start = match align {
                    Align::Start => px,
                    Align::Middle => px - width / 2.0,
                };
                // From the line's middle down to where its letters sit.
                page.text(document, start, py + size * 0.35, font, size, paper_colour(*paint).unwrap_or(crate::pdf::Rgb::BLACK), text);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(width: f32) -> Node {
        Node { width, height: 30.0, group: None }
    }

    fn edge(from: usize, to: usize) -> Edge {
        Edge { from, to, label: None }
    }

    #[test]
    fn layers_follow_the_longest_path_and_go_the_way_asked() {
        // a -> b -> c, and a -> c straight.
        let nodes = vec![node(40.0), node(40.0), node(40.0)];
        let edges = vec![edge(0, 1), edge(1, 2), edge(0, 2)];
        let down = layered(&nodes, &edges, Direction::Down, &Gaps::default());
        let (a, b, c) = (down.centres[0], down.centres[1], down.centres[2]);
        assert!(a.1 < b.1 && b.1 < c.1, "each a layer further down: {:?}", down.centres);
        // The long line passes a stand-in for each layer it crosses, so it has more points.
        assert!(down.routes[2].len() > 2);
        assert_eq!(down.routes[0].first(), Some(&a));
        assert_eq!(down.routes[0].last(), Some(&b));
        let right = layered(&nodes, &edges, Direction::Right, &Gaps::default());
        assert!(right.centres[0].0 < right.centres[1].0 && right.centres[1].0 < right.centres[2].0);
        let up = layered(&nodes, &edges, Direction::Up, &Gaps::default());
        assert!(up.centres[0].1 > up.centres[2].1);
    }

    #[test]
    fn loops_are_laid_out_and_their_lines_still_go_the_right_way() {
        let nodes = vec![node(40.0), node(40.0), node(40.0)];
        let edges = vec![edge(0, 1), edge(1, 2), edge(2, 0), edge(1, 1)];
        let layout = layered(&nodes, &edges, Direction::Down, &Gaps::default());
        assert_eq!(layout.routes[2].first(), Some(&layout.centres[2]), "the line back up starts at its own box");
        assert_eq!(layout.routes[2].last(), Some(&layout.centres[0]));
        assert!(layout.routes[3].len() >= 4, "a box's line to itself goes round its corner");
    }

    #[test]
    fn crossings_are_taken_out_where_they_can_be() {
        // Two pairs given crossed: a->d, b->c; drawn with c and d swapped round.
        let nodes = vec![node(30.0); 4];
        let edges = vec![edge(0, 3), edge(1, 2)];
        let layout = layered(&nodes, &edges, Direction::Down, &Gaps::default());
        let (a, b, c, d) = (layout.centres[0].0, layout.centres[1].0, layout.centres[2].0, layout.centres[3].0);
        assert_eq!(a < b, d < c, "the lines don't cross: {:?}", layout.centres);
    }

    #[test]
    fn boxes_in_a_layer_never_overlap_and_a_lone_child_sits_under_its_parent() {
        let nodes = vec![node(120.0), node(60.0), node(200.0), node(30.0), node(80.0)];
        let edges = vec![edge(0, 1), edge(0, 2), edge(0, 3), edge(0, 4)];
        let layout = layered(&nodes, &edges, Direction::Down, &Gaps::default());
        let mut row: Vec<(f32, f32)> = (1..5).map(|n| (layout.centres[n].0, nodes[n].width)).collect();
        row.sort_by(|a, b| a.0.total_cmp(&b.0));
        for pair in row.windows(2) {
            assert!(pair[1].0 - pair[0].0 >= (pair[0].1 + pair[1].1) / 2.0 + 29.0, "{row:?}");
        }
        let single = layered(&[node(100.0), node(30.0), node(300.0)], &[edge(0, 1)], Direction::Down, &Gaps::default());
        assert!((single.centres[0].0 - single.centres[1].0).abs() < 1.0, "{:?}", single.centres);
    }

    #[test]
    fn labels_get_a_place_between_the_layers() {
        let nodes = vec![node(40.0), node(40.0)];
        let edges = vec![Edge { from: 0, to: 1, label: Some((60.0, 14.0)) }];
        let layout = layered(&nodes, &edges, Direction::Down, &Gaps::default());
        let (x, y) = layout.labels[0].unwrap();
        assert!(y > layout.centres[0].1 + 15.0 && y < layout.centres[1].1 - 15.0);
        assert!((x - layout.centres[0].0).abs() < 1.0);
    }

    #[test]
    fn a_group_keeps_its_boxes_together() {
        // Top feeds four, of which the first and third are in a group.
        let mut nodes = vec![node(40.0); 5];
        nodes[1].group = Some(0);
        nodes[3].group = Some(0);
        let edges = vec![edge(0, 1), edge(0, 2), edge(0, 3), edge(0, 4)];
        let layout = layered(&nodes, &edges, Direction::Down, &Gaps::default());
        let mut row: Vec<(f32, usize)> = (1..5).map(|n| (layout.centres[n].0, n)).collect();
        row.sort_by(|a, b| a.0.total_cmp(&b.0));
        let places: Vec<usize> = row.iter().map(|(_, n)| *n).collect();
        let first = places.iter().position(|n| *n == 1).unwrap();
        let third = places.iter().position(|n| *n == 3).unwrap();
        assert_eq!(first.abs_diff(third), 1, "{places:?}");
    }

    #[test]
    fn lines_leave_from_a_shapes_edge() {
        assert_eq!(edge_point(Shape::Rect, 0.0, 0.0, 40.0, 20.0, (100.0, 0.0)), (20.0, 0.0));
        let (x, y) = edge_point(Shape::Circle, 0.0, 0.0, 20.0, 20.0, (10.0, 10.0));
        assert!(((x * x + y * y).sqrt() - 10.0).abs() < 0.01);
        assert_eq!(edge_point(Shape::Diamond, 0.0, 0.0, 40.0, 20.0, (0.0, 50.0)), (0.0, 10.0));
    }

    #[test]
    fn a_scene_fits_round_what_it_holds() {
        let mut scene = Scene::default();
        scene.items.push(Item::Rect { x: -10.0, y: 5.0, w: 20.0, h: 10.0, radius: 0.0, fill: Paint::Fill, stroke: Paint::Line, dashed: false });
        scene.fit(4.0);
        assert_eq!((scene.width, scene.height), (28.0, 18.0));
        assert_eq!(scene.items[0], Item::Rect { x: 4.0, y: 4.0, w: 20.0, h: 10.0, radius: 0.0, fill: Paint::Fill, stroke: Paint::Line, dashed: false });
        assert_eq!(smooth(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]).first(), Some(&(0.0, 0.0)));
        assert_eq!(smooth(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]).last(), Some(&(10.0, 10.0)));
    }
}
