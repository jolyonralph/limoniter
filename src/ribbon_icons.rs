//! The pictures on the ribbon's buttons and the start page's links, drawn with lines and shapes rather than taken
//! from a font, so they look the same on every system and take the window theme's colours.

use eframe::egui::{self, Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};

/// A picture for a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    New,
    Open,
    Save,
    SaveAs,
    Print,
    Cut,
    Copy,
    Paste,
    Undo,
    Redo,
    SelectAll,
    Find,
    Replace,
    GoTo,
    Up,
    Down,
    Back,
    Forward,
    Wrap,
    Map,
    Lines,
    Indent,
    Outdent,
    Sort,
    Table,
    Bookmark,
    Tag,
    Columns,
    Insert,
    Panel,
    Split,
    Terminal,
    Theme,
    Settings,
    Eye,
    Run,
    Stop,
    Debug,
    Build,
    Test,
    Bulb,
    Git,
    Compare,
    Merge,
    Download,
    Web,
    Server,
    Disc,
    Chip,
    Sprite,
    Tiles,
    Picture,
    Sound,
    Record,
    Script,
    Clock,
    Star,
    More,
    AlignLeft,
    AlignCentre,
    AlignRight,
    Justify,
    Pin,
    FullScreen,
    Window,
    Ribbon,
    Fold,
    Narrow,
    Assistant,
    Send,
    Lock,
    Book,
    Puzzle,
    Plus,
    Minus,
    /// A few letters or signs, as a badge is drawn: never words, which would need translating.
    Text(&'static str),
}

/// Draws an icon in a square, in one colour, with the accent for the part that stands out.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, ink: Color32, accent: Color32, back: Color32) {
    let side = rect.width().min(rect.height());
    let square = Rect::from_center_size(rect.center(), vec2(side, side));
    let at = |x: f32, y: f32| pos2(square.min.x + x * side, square.min.y + y * side);
    let width = (side / 13.0).max(1.0);
    let line = Stroke::new(width, ink);
    let bold = Stroke::new(width * 1.6, ink);
    let bright = Stroke::new(width * 1.4, accent);
    let poly = |points: &[(f32, f32)], fill: Color32, stroke: Stroke| {
        painter.add(Shape::convex_polygon(points.iter().map(|(x, y)| at(*x, *y)).collect(), fill, stroke));
    };
    let path = |points: &[(f32, f32)], stroke: Stroke| {
        painter.add(Shape::line(points.iter().map(|(x, y)| at(*x, *y)).collect(), stroke));
    };
    let seg = |a: (f32, f32), b: (f32, f32), stroke: Stroke| {
        painter.line_segment([at(a.0, a.1), at(b.0, b.1)], stroke);
    };
    let frame = |x0: f32, y0: f32, x1: f32, y1: f32, fill: Color32, stroke: Stroke| {
        let r = Rect::from_min_max(at(x0, y0), at(x1, y1));
        painter.rect(r, side * 0.06, fill, stroke, StrokeKind::Middle);
    };
    let circle = |x: f32, y: f32, r: f32, stroke: Stroke| {
        painter.circle_stroke(at(x, y), r * side, stroke);
    };
    let dot = |x: f32, y: f32, r: f32, colour: Color32| {
        painter.circle_filled(at(x, y), r * side, colour);
    };
    let arc = |cx: f32, cy: f32, r: f32, from: f32, to: f32, stroke: Stroke| {
        let steps = 14;
        let points: Vec<Pos2> = (0..=steps).map(|i| from + (to - from) * i as f32 / steps as f32).map(|a: f32| at(cx + r * a.to_radians().cos(), cy + r * a.to_radians().sin())).collect();
        painter.add(Shape::line(points, stroke));
    };
    // An arrowhead at a point, pointing along a direction given in degrees (0 right, 90 down).
    let head = |x: f32, y: f32, angle: f32, stroke: Stroke| {
        for turn in [150.0f32, -150.0] {
            let a = (angle + turn).to_radians();
            seg((x, y), (x + 0.2 * a.cos(), y + 0.2 * a.sin()), stroke);
        }
    };
    let page = |x0: f32, y0: f32, x1: f32, y1: f32| {
        let fold = (x1 - x0) * 0.32;
        poly(&[(x0, y0), (x1 - fold, y0), (x1, y0 + fold), (x1, y1), (x0, y1)], back, line);
        path(&[(x1 - fold, y0), (x1 - fold, y0 + fold), (x1, y0 + fold)], line);
    };
    let lines = |x0: f32, x1: f32, rows: &[(f32, f32, f32)]| {
        for (y, left, right) in rows {
            seg((x0 + left, *y), (x1 - right, *y), bold);
        }
    };
    let floppy = |x0: f32, y0: f32, x1: f32, y1: f32| {
        let w = x1 - x0;
        let h = y1 - y0;
        poly(&[(x0, y0), (x1 - w * 0.18, y0), (x1, y0 + h * 0.18), (x1, y1), (x0, y1)], back, line);
        frame(x0 + w * 0.22, y0, x1 - w * 0.3, y0 + h * 0.3, Color32::TRANSPARENT, line);
        frame(x0 + w * 0.18, y0 + h * 0.55, x1 - w * 0.18, y1, accent, Stroke::NONE);
    };
    let magnifier = |x: f32, y: f32, r: f32| {
        circle(x, y, r, bold);
        seg((x + r * 0.72, y + r * 0.72), (x + r * 1.75, y + r * 1.75), Stroke::new(width * 2.4, ink));
    };
    let window = |fill: Color32| frame(0.1, 0.18, 0.9, 0.82, fill, line);
    match icon {
        Icon::New => {
            page(0.2, 0.08, 0.76, 0.92);
            seg((0.62, 0.74), (0.94, 0.74), bright);
            seg((0.78, 0.58), (0.78, 0.9), bright);
        }
        Icon::Open => {
            poly(&[(0.08, 0.22), (0.38, 0.22), (0.46, 0.32), (0.9, 0.32), (0.9, 0.82), (0.08, 0.82)], back, line);
            poly(&[(0.08, 0.82), (0.2, 0.45), (0.98, 0.45), (0.86, 0.82)], accent, Stroke::NONE);
        }
        Icon::Save => floppy(0.14, 0.14, 0.86, 0.86),
        Icon::SaveAs => {
            floppy(0.06, 0.06, 0.7, 0.7);
            seg((0.5, 0.95), (0.95, 0.5), Stroke::new(width * 2.4, accent));
        }
        Icon::Print => {
            frame(0.28, 0.1, 0.72, 0.4, back, line);
            frame(0.1, 0.36, 0.9, 0.72, back, line);
            frame(0.28, 0.6, 0.72, 0.9, back, line);
            dot(0.78, 0.48, 0.035, accent);
        }
        Icon::Cut => {
            circle(0.3, 0.76, 0.12, bold);
            circle(0.7, 0.76, 0.12, bold);
            seg((0.38, 0.66), (0.72, 0.08), bold);
            seg((0.62, 0.66), (0.28, 0.08), bold);
        }
        Icon::Copy => {
            frame(0.12, 0.08, 0.62, 0.66, back, line);
            frame(0.38, 0.34, 0.88, 0.92, back, line);
            lines(0.38, 0.88, &[(0.52, 0.1, 0.1), (0.66, 0.1, 0.1), (0.8, 0.1, 0.22)]);
        }
        Icon::Paste => {
            frame(0.16, 0.16, 0.84, 0.92, back, line);
            frame(0.34, 0.06, 0.66, 0.24, accent, Stroke::NONE);
            lines(0.16, 0.84, &[(0.46, 0.14, 0.14), (0.6, 0.14, 0.14), (0.74, 0.14, 0.28)]);
        }
        Icon::Undo | Icon::Redo => {
            let flip = |x: f32| if icon == Icon::Redo { 1.0 - x } else { x };
            let steps = 14;
            let points: Vec<Pos2> = (0..=steps)
                .map(|i| (30.0 - 210.0 * i as f32 / steps as f32).to_radians())
                .map(|a| at(flip(0.52 + 0.3 * a.cos()), 0.58 + 0.3 * a.sin()))
                .collect();
            painter.add(Shape::line(points, bold));
            let (x, y) = (flip(0.22), 0.58);
            seg((x, y), (flip(0.08), 0.42), bold);
            seg((x, y), (flip(0.38), 0.46), bold);
        }
        Icon::SelectAll => {
            let r = Rect::from_min_max(at(0.1, 0.12), at(0.9, 0.88));
            let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.extend(Shape::dashed_line(&corners, line, side * 0.1, side * 0.07));
            lines(0.1, 0.9, &[(0.36, 0.14, 0.14), (0.5, 0.14, 0.14), (0.64, 0.14, 0.3)]);
        }
        Icon::Find => magnifier(0.4, 0.4, 0.24),
        Icon::Replace => {
            path(&[(0.12, 0.34), (0.82, 0.34)], bold);
            head(0.86, 0.34, 0.0, bold);
            path(&[(0.88, 0.68), (0.18, 0.68)], Stroke::new(width * 1.6, accent));
            head(0.14, 0.68, 180.0, Stroke::new(width * 1.6, accent));
        }
        Icon::GoTo => {
            seg((0.1, 0.5), (0.68, 0.5), bold);
            head(0.7, 0.5, 0.0, bold);
            seg((0.86, 0.16), (0.86, 0.84), bright);
        }
        Icon::Up | Icon::Down | Icon::Back | Icon::Forward => {
            let angle = match icon {
                Icon::Up => 270.0f32,
                Icon::Down => 90.0,
                Icon::Back => 180.0,
                _ => 0.0,
            };
            let (dx, dy) = (angle.to_radians().cos() * 0.34, angle.to_radians().sin() * 0.34);
            seg((0.5 - dx, 0.5 - dy), (0.5 + dx, 0.5 + dy), bold);
            head(0.5 + dx, 0.5 + dy, angle, bold);
        }
        Icon::Wrap => {
            seg((0.1, 0.22), (0.9, 0.22), bold);
            path(&[(0.1, 0.48), (0.78, 0.48)], bold);
            arc(0.78, 0.6, 0.12, -90.0, 90.0, Stroke::new(width * 1.6, accent));
            seg((0.78, 0.72), (0.5, 0.72), Stroke::new(width * 1.6, accent));
            head(0.48, 0.72, 180.0, Stroke::new(width * 1.6, accent));
            seg((0.1, 0.72), (0.34, 0.72), bold);
        }
        Icon::Map => {
            frame(0.24, 0.08, 0.76, 0.92, back, line);
            frame(0.24, 0.38, 0.76, 0.6, accent.gamma_multiply(0.5), Stroke::NONE);
            lines(0.24, 0.76, &[(0.2, 0.08, 0.12), (0.3, 0.08, 0.2), (0.48, 0.08, 0.08), (0.7, 0.08, 0.16), (0.8, 0.08, 0.24)]);
        }
        Icon::Lines => lines(0.1, 0.9, &[(0.2, 0.0, 0.0), (0.4, 0.0, 0.15), (0.6, 0.0, 0.0), (0.8, 0.0, 0.3)]),
        Icon::Indent | Icon::Outdent => {
            lines(0.1, 0.9, &[(0.14, 0.0, 0.0), (0.38, 0.4, 0.0), (0.62, 0.4, 0.0), (0.86, 0.0, 0.0)]);
            if icon == Icon::Indent {
                poly(&[(0.08, 0.34), (0.3, 0.5), (0.08, 0.66)], accent, Stroke::NONE);
            } else {
                poly(&[(0.3, 0.34), (0.08, 0.5), (0.3, 0.66)], accent, Stroke::NONE);
            }
        }
        Icon::Sort => {
            for (y, right) in [(0.2, 0.62), (0.4, 0.5), (0.6, 0.38), (0.8, 0.26)] {
                seg((0.08, y), (0.08 + right, y), bold);
            }
            seg((0.84, 0.14), (0.84, 0.84), bright);
            head(0.84, 0.86, 90.0, bright);
        }
        Icon::Table => {
            frame(0.08, 0.16, 0.92, 0.84, back, line);
            frame(0.08, 0.16, 0.92, 0.36, accent.gamma_multiply(0.6), line);
            seg((0.08, 0.6), (0.92, 0.6), line);
            for x in [0.36, 0.64] {
                seg((x, 0.16), (x, 0.84), line);
            }
        }
        Icon::Bookmark => poly(&[(0.28, 0.08), (0.72, 0.08), (0.72, 0.92), (0.5, 0.7), (0.28, 0.92)], accent, line),
        Icon::Tag => {
            poly(&[(0.08, 0.3), (0.62, 0.3), (0.92, 0.5), (0.62, 0.7), (0.08, 0.7)], accent, line);
            dot(0.62, 0.5, 0.05, back);
        }
        Icon::Columns => {
            for x in [0.16, 0.5, 0.84] {
                seg((x, 0.1), (x, 0.9), bold);
            }
            frame(0.3, 0.3, 0.7, 0.7, accent.gamma_multiply(0.5), Stroke::NONE);
        }
        Icon::Insert => {
            seg((0.3, 0.12), (0.3, 0.88), bold);
            seg((0.2, 0.12), (0.4, 0.12), line);
            seg((0.2, 0.88), (0.4, 0.88), line);
            seg((0.52, 0.5), (0.9, 0.5), bright);
            seg((0.71, 0.31), (0.71, 0.69), bright);
        }
        Icon::Panel => {
            window(back);
            frame(0.1, 0.18, 0.36, 0.82, accent.gamma_multiply(0.6), line);
        }
        Icon::Split => {
            window(back);
            seg((0.5, 0.18), (0.5, 0.82), bright);
        }
        Icon::Terminal => {
            frame(0.08, 0.16, 0.92, 0.84, ink, Stroke::NONE);
            path(&[(0.22, 0.34), (0.38, 0.5), (0.22, 0.66)], Stroke::new(width * 1.6, back));
            seg((0.46, 0.68), (0.72, 0.68), Stroke::new(width * 1.6, accent));
        }
        Icon::Theme => {
            circle(0.5, 0.5, 0.38, bold);
            let points: Vec<Pos2> = (0..=16).map(|i| (-90.0 + 180.0 * i as f32 / 16.0).to_radians()).map(|a| at(0.5 + 0.38 * a.cos(), 0.5 + 0.38 * a.sin())).collect();
            painter.add(Shape::convex_polygon(points, accent, Stroke::NONE));
        }
        Icon::Settings => {
            for i in 0..8 {
                let a = (i as f32 * 45.0).to_radians();
                seg((0.5 + 0.26 * a.cos(), 0.5 + 0.26 * a.sin()), (0.5 + 0.42 * a.cos(), 0.5 + 0.42 * a.sin()), Stroke::new(width * 2.6, ink));
            }
            painter.circle(at(0.5, 0.5), 0.27 * side, back, bold);
            dot(0.5, 0.5, 0.09, accent);
        }
        Icon::Eye => {
            arc(0.5, 0.95, 0.6, 222.0, 318.0, bold);
            arc(0.5, 0.05, 0.6, 42.0, 138.0, bold);
            dot(0.5, 0.5, 0.14, accent);
        }
        Icon::Run => poly(&[(0.26, 0.14), (0.84, 0.5), (0.26, 0.86)], accent, Stroke::NONE),
        Icon::Stop => frame(0.22, 0.22, 0.78, 0.78, accent, Stroke::NONE),
        Icon::Debug => {
            poly(&[(0.14, 0.14), (0.62, 0.44), (0.14, 0.74)], accent, Stroke::NONE);
            painter.circle(at(0.7, 0.7), 0.17 * side, back, bold);
        }
        Icon::Build => {
            frame(0.1, 0.54, 0.48, 0.88, back, line);
            frame(0.52, 0.54, 0.9, 0.88, back, line);
            frame(0.3, 0.16, 0.7, 0.5, accent, line);
        }
        Icon::Test => path(&[(0.14, 0.52), (0.4, 0.78), (0.88, 0.22)], Stroke::new(width * 2.2, accent)),
        Icon::Bulb => {
            painter.circle(at(0.5, 0.4), 0.27 * side, accent, line);
            frame(0.38, 0.66, 0.62, 0.9, back, line);
        }
        Icon::Git => {
            seg((0.3, 0.2), (0.3, 0.8), bold);
            path(&[(0.3, 0.66), (0.7, 0.44), (0.7, 0.3)], bold);
            for (x, y) in [(0.3, 0.16), (0.3, 0.84), (0.7, 0.26)] {
                painter.circle(at(x, y), 0.1 * side, accent, line);
            }
        }
        Icon::Compare => {
            frame(0.06, 0.1, 0.46, 0.9, back, line);
            frame(0.54, 0.1, 0.94, 0.9, back, line);
            frame(0.06, 0.42, 0.46, 0.56, accent.gamma_multiply(0.6), Stroke::NONE);
            frame(0.54, 0.42, 0.94, 0.56, accent.gamma_multiply(0.6), Stroke::NONE);
        }
        Icon::Merge => {
            path(&[(0.2, 0.1), (0.5, 0.46), (0.8, 0.1)], bold);
            seg((0.5, 0.46), (0.5, 0.86), bright);
            head(0.5, 0.9, 90.0, bright);
        }
        Icon::Download => {
            seg((0.5, 0.08), (0.5, 0.6), bright);
            head(0.5, 0.64, 90.0, bright);
            path(&[(0.12, 0.6), (0.12, 0.88), (0.88, 0.88), (0.88, 0.6)], bold);
        }
        Icon::Web => {
            circle(0.5, 0.5, 0.4, bold);
            arc(0.5, 0.5, 0.4, -90.0, 90.0, line);
            arc(1.1, 0.5, 0.6, 140.0, 220.0, line);
            arc(-0.1, 0.5, 0.6, -40.0, 40.0, line);
            seg((0.1, 0.5), (0.9, 0.5), line);
        }
        Icon::Server => {
            frame(0.12, 0.12, 0.88, 0.44, back, line);
            frame(0.12, 0.56, 0.88, 0.88, back, line);
            dot(0.26, 0.28, 0.045, accent);
            dot(0.26, 0.72, 0.045, accent);
        }
        Icon::Disc => {
            painter.circle(at(0.5, 0.5), 0.4 * side, back, bold);
            circle(0.5, 0.5, 0.25, Stroke::new(width, accent));
            dot(0.5, 0.5, 0.08, ink);
        }
        Icon::Chip => {
            frame(0.24, 0.24, 0.76, 0.76, accent, line);
            for t in [0.34, 0.5, 0.66] {
                seg((t, 0.08), (t, 0.24), line);
                seg((t, 0.76), (t, 0.92), line);
                seg((0.08, t), (0.24, t), line);
                seg((0.76, t), (0.92, t), line);
            }
        }
        Icon::Sprite => {
            // A little invader, a cell at a time.
            let rows = ["0010100", "0111110", "1101011", "1111111", "1011101", "1000001"];
            let cell = 0.84 / 7.0;
            for (y, row) in rows.iter().enumerate() {
                for (x, on) in row.chars().enumerate() {
                    if on == '1' {
                        let x0 = 0.08 + x as f32 * cell;
                        let y0 = 0.16 + y as f32 * cell;
                        frame(x0, y0, x0 + cell, y0 + cell, if y == 2 { accent } else { ink }, Stroke::NONE);
                    }
                }
            }
        }
        Icon::Tiles => {
            for (x, y) in [(0, 0), (1, 0), (2, 0), (0, 1), (2, 1), (0, 2), (1, 2), (2, 2)] {
                let (x0, y0) = (0.08 + x as f32 * 0.28, 0.08 + y as f32 * 0.28);
                frame(x0, y0, x0 + 0.26, y0 + 0.26, if (x + y) % 2 == 0 { accent } else { ink }, Stroke::NONE);
            }
        }
        Icon::Picture => {
            frame(0.08, 0.14, 0.92, 0.86, back, line);
            poly(&[(0.12, 0.82), (0.4, 0.44), (0.62, 0.7), (0.72, 0.58), (0.88, 0.82)], accent, Stroke::NONE);
            dot(0.7, 0.34, 0.08, accent);
        }
        Icon::Sound => {
            poly(&[(0.1, 0.38), (0.3, 0.38), (0.52, 0.16), (0.52, 0.84), (0.3, 0.62), (0.1, 0.62)], ink, Stroke::NONE);
            arc(0.52, 0.5, 0.18, -50.0, 50.0, bright);
            arc(0.52, 0.5, 0.34, -50.0, 50.0, bright);
        }
        Icon::Record => {
            circle(0.5, 0.5, 0.38, line);
            dot(0.5, 0.5, 0.26, Color32::from_rgb(220, 60, 60));
        }
        Icon::Script => {
            page(0.16, 0.08, 0.84, 0.92);
            painter.text(at(0.5, 0.6), egui::Align2::CENTER_CENTER, "{}", egui::FontId::monospace(side * 0.36), accent);
        }
        Icon::Clock => {
            circle(0.5, 0.5, 0.4, bold);
            seg((0.5, 0.5), (0.5, 0.24), bold);
            seg((0.5, 0.5), (0.7, 0.6), bright);
        }
        Icon::Star => {
            let points: Vec<Pos2> = (0..10).map(|i| {
                let a = (-90.0 + i as f32 * 36.0).to_radians();
                let r = if i % 2 == 0 { 0.44 } else { 0.18 };
                at(0.5 + r * a.cos(), 0.54 + r * a.sin())
            }).collect();
            painter.add(Shape::closed_line(points.clone(), line));
            // Filled from the middle, a triangle a point, as the outline isn't convex.
            for pair in 0..10 {
                painter.add(Shape::convex_polygon(vec![at(0.5, 0.54), points[pair], points[(pair + 1) % 10]], accent, Stroke::NONE));
            }
        }
        Icon::More => {
            for x in [0.24, 0.5, 0.76] {
                dot(x, 0.5, 0.075, ink);
            }
        }
        Icon::AlignLeft => lines(0.1, 0.9, &[(0.2, 0.0, 0.0), (0.4, 0.0, 0.3), (0.6, 0.0, 0.0), (0.8, 0.0, 0.4)]),
        Icon::AlignCentre => lines(0.1, 0.9, &[(0.2, 0.0, 0.0), (0.4, 0.15, 0.15), (0.6, 0.0, 0.0), (0.8, 0.2, 0.2)]),
        Icon::AlignRight => lines(0.1, 0.9, &[(0.2, 0.0, 0.0), (0.4, 0.3, 0.0), (0.6, 0.0, 0.0), (0.8, 0.4, 0.0)]),
        Icon::Justify => lines(0.1, 0.9, &[(0.2, 0.0, 0.0), (0.4, 0.0, 0.0), (0.6, 0.0, 0.0), (0.8, 0.0, 0.0)]),
        Icon::Pin => {
            painter.circle(at(0.5, 0.32), 0.2 * side, accent, line);
            seg((0.5, 0.52), (0.5, 0.92), bold);
        }
        Icon::FullScreen => {
            for (x, y, dx, dy) in [(0.12, 0.12, 1.0, 1.0), (0.88, 0.12, -1.0, 1.0), (0.12, 0.88, 1.0, -1.0), (0.88, 0.88, -1.0, -1.0)] {
                path(&[(x + dx * 0.26, y), (x, y), (x, y + dy * 0.26)], bold);
            }
        }
        Icon::Window => {
            window(back);
            frame(0.1, 0.18, 0.9, 0.34, accent, line);
        }
        Icon::Ribbon => {
            frame(0.08, 0.16, 0.92, 0.84, back, line);
            frame(0.08, 0.16, 0.92, 0.36, accent, line);
            for x in [0.2, 0.44, 0.68] {
                frame(x, 0.48, x + 0.14, 0.62, ink, Stroke::NONE);
            }
        }
        Icon::Fold => {
            path(&[(0.2, 0.62), (0.5, 0.32), (0.8, 0.62)], bold);
        }
        Icon::Narrow => {
            seg((0.1, 0.12), (0.9, 0.12), line);
            seg((0.1, 0.88), (0.9, 0.88), line);
            frame(0.1, 0.38, 0.9, 0.62, accent.gamma_multiply(0.6), Stroke::NONE);
            head(0.5, 0.34, 90.0, bold);
            head(0.5, 0.66, 270.0, bold);
        }
        Icon::Assistant => {
            for (x, y, r) in [(0.42, 0.5, 0.34), (0.78, 0.22, 0.14)] {
                let points: Vec<Pos2> = (0..8).map(|i| {
                    let a = (i as f32 * 45.0).to_radians();
                    let reach = if i % 2 == 0 { r } else { r * 0.3 };
                    at(x + reach * a.cos(), y + reach * a.sin())
                }).collect();
                for pair in 0..8 {
                    painter.add(Shape::convex_polygon(vec![at(x, y), points[pair], points[(pair + 1) % 8]], accent, Stroke::NONE));
                }
            }
        }
        Icon::Send => {
            frame(0.08, 0.36, 0.6, 0.9, back, line);
            seg((0.34, 0.62), (0.84, 0.12), bright);
            head(0.86, 0.1, 315.0, bright);
        }
        Icon::Lock => {
            arc(0.5, 0.42, 0.2, 180.0, 360.0, bold);
            seg((0.3, 0.42), (0.3, 0.5), bold);
            seg((0.7, 0.42), (0.7, 0.5), bold);
            frame(0.2, 0.48, 0.8, 0.9, accent, line);
        }
        Icon::Book => {
            poly(&[(0.08, 0.18), (0.48, 0.24), (0.48, 0.88), (0.08, 0.82)], back, line);
            poly(&[(0.52, 0.24), (0.92, 0.18), (0.92, 0.82), (0.52, 0.88)], back, line);
            lines(0.52, 0.92, &[(0.42, 0.08, 0.08), (0.58, 0.08, 0.14)]);
            seg((0.16, 0.4), (0.4, 0.43), bright);
        }
        Icon::Puzzle => {
            frame(0.12, 0.24, 0.76, 0.88, back, line);
            painter.circle(at(0.44, 0.2), 0.1 * side, accent, line);
            painter.circle(at(0.8, 0.56), 0.1 * side, accent, line);
        }
        Icon::Plus | Icon::Minus => {
            frame(0.14, 0.14, 0.86, 0.86, back, line);
            seg((0.3, 0.5), (0.7, 0.5), bold);
            if icon == Icon::Plus {
                seg((0.5, 0.3), (0.5, 0.7), bold);
            }
        }
        Icon::Text(letters) => {
            let size = side * if letters.chars().count() >= 3 { 0.46 } else { 0.62 };
            let font = egui::FontId::proportional(size);
            // Drawn twice, a hair apart, to stand as firmly as the drawn icons beside them.
            for nudge in [0.0, side / 26.0] {
                painter.text(at(0.5, 0.5) + vec2(nudge, 0.0), egui::Align2::CENTER_CENTER, letters, font.clone(), ink);
            }
            seg((0.2, 0.9), (0.8, 0.9), bright);
        }
    }
}

/// A small arrow pointing down, after the name of a button that opens a list.
pub fn chevron(painter: &Painter, centre: Pos2, size: f32, ink: Color32) {
    let half = size / 2.0;
    painter.add(Shape::line(vec![centre + vec2(-half, -half / 2.0), centre + vec2(0.0, half / 2.0), centre + vec2(half, -half / 2.0)], Stroke::new(1.2, ink)));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon draws something, inside its square, at the sizes the ribbon and start page use.
    #[test]
    fn every_icon_draws_inside_its_square() {
        let ctx = egui::Context::default();
        let icons = [
            Icon::New, Icon::Open, Icon::Save, Icon::SaveAs, Icon::Print, Icon::Cut, Icon::Copy, Icon::Paste, Icon::Undo, Icon::Redo, Icon::SelectAll, Icon::Find, Icon::Replace,
            Icon::GoTo, Icon::Up, Icon::Down, Icon::Back, Icon::Forward, Icon::Wrap, Icon::Map, Icon::Lines, Icon::Indent, Icon::Outdent, Icon::Sort, Icon::Table, Icon::Bookmark,
            Icon::Tag, Icon::Columns, Icon::Insert, Icon::Panel, Icon::Split, Icon::Terminal, Icon::Theme, Icon::Settings, Icon::Eye, Icon::Run, Icon::Stop, Icon::Debug, Icon::Build,
            Icon::Test, Icon::Bulb, Icon::Git, Icon::Compare, Icon::Merge, Icon::Download, Icon::Web, Icon::Server, Icon::Disc, Icon::Chip, Icon::Sprite, Icon::Tiles, Icon::Picture,
            Icon::Sound, Icon::Record, Icon::Script, Icon::Clock, Icon::Star, Icon::More, Icon::AlignLeft, Icon::AlignCentre, Icon::AlignRight, Icon::Justify, Icon::Pin,
            Icon::FullScreen, Icon::Window, Icon::Ribbon, Icon::Fold, Icon::Narrow, Icon::Assistant, Icon::Send, Icon::Lock, Icon::Book, Icon::Puzzle, Icon::Plus, Icon::Minus,
            Icon::Text("Aa"),
        ];
        for size in [16.0, 26.0] {
            for icon in icons {
                let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(200.0, 200.0))), ..Default::default() };
                let mut output = ctx.run_ui(raw, |ui| {
                    let square = Rect::from_min_size(pos2(50.0, 50.0), vec2(size, size));
                    paint(ui.painter(), square, icon, Color32::BLACK, Color32::RED, Color32::WHITE);
                });
                output.textures_delta.clear();
                let shapes: Vec<_> = output.shapes.iter().filter(|s| !matches!(s.shape, Shape::Noop)).collect();
                assert!(!shapes.is_empty(), "{icon:?} draws nothing");
                let bounds = shapes.iter().fold(Rect::NOTHING, |r, s| r.union(s.shape.visual_bounding_rect()));
                // A little over the square for the width of a line, but no further.
                let allowed = Rect::from_min_size(pos2(50.0, 50.0), vec2(size, size)).expand(size * 0.2);
                assert!(allowed.contains_rect(bounds), "{icon:?} at {size} reaches {bounds:?}");
            }
        }
    }
}
