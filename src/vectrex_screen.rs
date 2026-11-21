//! The Vectrex's screen: where its beam is, worked out from the integrators as the VIA and the DAC drive them, the
//! lines it draws while it's lit, and those lines glowing on a phosphor that fades between one refresh and the next.
//!
//! How the beam moves (the Vectrex Programmer's Manual's hardware description, and vecx's analog model, which the
//! community's emulators follow): the DAC on the VIA's port A drives the X integrator directly, and through the
//! multiplexer (port B bits 1 and 2, enabled while bit 0 is low) it sets the sample-and-holds for Y, for the zero
//! reference both axes are measured from, and for the brightness. While RAMP (PB7, or timer 1 driving it) is low,
//! each integrator moves the beam by its input less the zero reference every cycle; holding ZERO (CA2) low puts the
//! beam back in the middle; and the beam is lit while BLANK (CB2, from the shift register when it's shifting) is
//! high and the brightness is above 0. The units are vecx's: the beam can reach 16,500 either side across and 20,500
//! up and down, which is the Vectrex's tall screen.

/// The picture's size: the screen is 33 wide to 41 high.
pub const WIDTH: usize = 400;
pub const HEIGHT: usize = 497;
/// How far the beam goes from the middle, in the integrators' units, to the screen's edges.
pub const HALF_X: i32 = 16_500;
pub const HALF_Y: i32 = 20_500;
/// The integrators can't swing further than this (they saturate), off the screen.
const LIMIT: i32 = 40_000;
/// What's left of a refresh's light at the next, a fiftieth of a second later.
const PERSIST: f32 = 0.35;
/// How bright the light a cycle of a fully bright beam leaves is.
const GAIN: f32 = 2.6;
/// At most this many lines are kept in a refresh, for a program that turns the beam on and off without end.
const MOST_SEGMENTS: usize = 100_000;

/// What the analog side is given in a cycle: the integrators' inputs, and the beam's controls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signals {
    /// ZERO held: the integrators are put back to the middle.
    pub zero: bool,
    /// RAMP active: the integrators integrate.
    pub ramp: bool,
    /// BLANK released: the beam can be seen.
    pub lit: bool,
    /// The integrators' inputs less the zero reference, the distance moved in a cycle.
    pub dx: i32,
    pub dy: i32,
    /// The brightness, 0 to 127.
    pub z: u8,
}

/// A line the beam drew, in the integrators' units (y up), with its brightness and how many cycles it took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub from: (i32, i32),
    pub to: (i32, i32),
    pub z: u8,
    pub cycles: u32,
}

#[derive(Clone, Copy, Debug)]
struct Stroke {
    from: (i32, i32),
    dx: i32,
    dy: i32,
    z: u8,
    cycles: u32,
}

/// The beam: where it is, and the lines it's drawn since the last refresh.
#[derive(Clone, Debug, Default)]
pub struct Beam {
    pub x: i32,
    pub y: i32,
    stroke: Option<Stroke>,
    pub segments: Vec<Segment>,
}

impl Beam {
    /// One cycle: the beam moves as the integrators say, and while it's lit the line it draws goes on (a new one
    /// begun when its speed, direction or brightness changes).
    pub fn tick(&mut self, signals: Signals) {
        if signals.zero {
            self.end();
            self.x = 0;
            self.y = 0;
            return;
        }
        let (dx, dy) = if signals.ramp { (signals.dx, signals.dy) } else { (0, 0) };
        if signals.lit && signals.z > 0 {
            let same = self.stroke.is_some_and(|s| s.dx == dx && s.dy == dy && s.z == signals.z);
            if !same {
                self.end();
                self.stroke = Some(Stroke { from: (self.x, self.y), dx, dy, z: signals.z, cycles: 0 });
            }
        } else {
            self.end();
        }
        self.x = (self.x + dx).clamp(-LIMIT, LIMIT);
        self.y = (self.y + dy).clamp(-LIMIT, LIMIT);
        if let Some(stroke) = &mut self.stroke {
            stroke.cycles += 1;
        }
    }

    /// The line being drawn, finished where the beam is.
    fn end(&mut self) {
        if let Some(stroke) = self.stroke.take()
            && self.segments.len() < MOST_SEGMENTS
        {
            self.segments.push(Segment { from: stroke.from, to: (self.x, self.y), z: stroke.z, cycles: stroke.cycles });
        }
    }

    /// The lines drawn so far, taken for a refresh; a line being drawn is split there, and carries on.
    pub fn take(&mut self) -> Vec<Segment> {
        if let Some(stroke) = self.stroke {
            self.end();
            self.stroke = Some(Stroke { from: (self.x, self.y), cycles: 0, ..stroke });
        }
        std::mem::take(&mut self.segments)
    }
}

/// Where a point in the integrators' units is on the picture, in pixels (fractional).
pub fn to_pixel(x: i32, y: i32) -> (f32, f32) {
    let px = (x + HALF_X) as f32 * WIDTH as f32 / (2 * HALF_X) as f32;
    let py = (HALF_Y - y) as f32 * HEIGHT as f32 / (2 * HALF_Y) as f32;
    (px, py)
}

/// The phosphor: the light on each pixel, which fades by each refresh.
#[derive(Clone)]
pub struct Screen {
    light: Vec<f32>,
    pub refreshes: u64,
}

impl Default for Screen {
    fn default() -> Screen {
        Screen { light: vec![0.0; WIDTH * HEIGHT], refreshes: 0 }
    }
}

impl Screen {
    /// A fiftieth of a second on: the light fades, and the lines drawn since the last refresh are added.
    pub fn refresh(&mut self, segments: &[Segment]) {
        for light in &mut self.light {
            *light *= PERSIST;
        }
        for segment in segments {
            self.draw(segment);
        }
        self.refreshes += 1;
    }

    /// A line's light: the beam's energy spread along it, so a slow line is brighter than a fast one, and a
    /// beam held still makes a bright dot.
    fn draw(&mut self, segment: &Segment) {
        let (x0, y0) = to_pixel(segment.from.0, segment.from.1);
        let (x1, y1) = to_pixel(segment.to.0, segment.to.1);
        let length = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
        let steps = (length * 2.0).ceil().max(1.0) as usize;
        let energy = f32::from(segment.z) / 127.0 * segment.cycles.max(1) as f32 * GAIN / steps as f32;
        for step in 0..steps {
            let t = (step as f32 + 0.5) / steps as f32;
            self.splat(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, energy);
        }
    }

    /// Light at a point between pixels, shared among the four around it.
    fn splat(&mut self, x: f32, y: f32, energy: f32) {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (ix, iy) = (fx.floor(), fy.floor());
        let (ax, ay) = (fx - ix, fy - iy);
        for (dx, dy, share) in [(0, 0, (1.0 - ax) * (1.0 - ay)), (1, 0, ax * (1.0 - ay)), (0, 1, (1.0 - ax) * ay), (1, 1, ax * ay)] {
            let (px, py) = (ix as i64 + dx, iy as i64 + dy);
            if px >= 0 && py >= 0 && (px as usize) < WIDTH && (py as usize) < HEIGHT {
                self.light[py as usize * WIDTH + px as usize] += energy * share;
            }
        }
    }

    /// The light on a pixel, for the tests.
    #[cfg(test)]
    pub fn light_at(&self, x: usize, y: usize) -> f32 {
        self.light[y * WIDTH + x]
    }

    /// The picture as RGBA: each line a white core, softened a little, in a wider blue-white glow, as the Vectrex's
    /// phosphor looks.
    pub fn picture(&self, out: &mut Vec<u8>) {
        let soft = blur(&self.light, 1);
        let glow = blur(&blur(&self.light, 4), 4);
        out.clear();
        out.reserve(WIDTH * HEIGHT * 4);
        for (core, halo) in soft.iter().zip(&glow) {
            let core = 1.0 - (-core * 1.4).exp();
            let halo = 1.0 - (-halo * 3.0).exp();
            let rgb = [core * 0.94 + halo * 0.30, core * 0.97 + halo * 0.45, core + halo * 0.75];
            out.extend(rgb.iter().map(|c| (c.clamp(0.0, 1.0) * 255.0) as u8));
            out.push(255);
        }
    }
}

/// A box blur of a radius, across and then down.
fn blur(light: &[f32], radius: usize) -> Vec<f32> {
    let across = blur_lines(light, WIDTH, HEIGHT, 1, WIDTH, radius);
    blur_lines(&across, HEIGHT, WIDTH, WIDTH, 1, radius)
}

/// Blurs each of `count` lines of `length` pixels, `step` apart along a line and `next` from one line to the next.
fn blur_lines(light: &[f32], length: usize, count: usize, step: usize, next: usize, radius: usize) -> Vec<f32> {
    let mut out = vec![0.0; light.len()];
    let scale = 1.0 / (2 * radius + 1) as f32;
    for line in 0..count {
        let at = |i: usize| line * next + i * step;
        let mut sum: f32 = (0..=radius.min(length - 1)).map(|i| light[at(i)]).sum();
        for i in 0..length {
            out[at(i)] = sum * scale;
            if i + radius + 1 < length {
                sum += light[at(i + radius + 1)];
            }
            if i >= radius {
                sum -= light[at(i - radius)];
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(beam: &mut Beam, signals: Signals, cycles: usize) {
        for _ in 0..cycles {
            beam.tick(signals);
        }
    }

    #[test]
    fn the_integrators_draw_a_known_line() {
        let mut beam = Beam { x: 1234, y: -99, ..Beam::default() };
        // Zeroed: back to the middle, drawing nothing.
        run(&mut beam, Signals { zero: true, lit: true, z: 127, ..Signals::default() }, 3);
        assert_eq!((beam.x, beam.y), (0, 0));
        assert!(beam.segments.is_empty());
        // A move with the beam dark: 50 cycles at (-20, 30) a cycle.
        run(&mut beam, Signals { ramp: true, dx: -20, dy: 30, ..Signals::default() }, 50);
        assert_eq!((beam.x, beam.y), (-1000, 1500));
        assert!(beam.segments.is_empty(), "dark");
        // Lit, 80 cycles at (100, -50), at full brightness: the line from where it was.
        run(&mut beam, Signals { ramp: true, lit: true, dx: 100, dy: -50, z: 127, ..Signals::default() }, 80);
        // RAMP off: it stays where it is, still lit, which is a dot (a line going nowhere).
        run(&mut beam, Signals { lit: true, dx: 100, dy: -50, z: 127, ..Signals::default() }, 5);
        run(&mut beam, Signals::default(), 1);
        assert_eq!(beam.segments, vec![Segment { from: (-1000, 1500), to: (7000, -2500), z: 127, cycles: 80 }, Segment { from: (7000, -2500), to: (7000, -2500), z: 127, cycles: 5 }]);
        // Brightness 0 draws nothing, even lit.
        run(&mut beam, Signals { ramp: true, lit: true, dx: 1, z: 0, ..Signals::default() }, 10);
        run(&mut beam, Signals::default(), 1);
        assert_eq!(beam.segments.len(), 2);
    }

    #[test]
    fn a_line_being_drawn_at_a_refresh_is_split_there() {
        let mut beam = Beam::default();
        let lit = Signals { ramp: true, lit: true, dx: 10, dy: 0, z: 64, ..Signals::default() };
        run(&mut beam, lit, 30);
        let first = beam.take();
        run(&mut beam, lit, 20);
        run(&mut beam, Signals::default(), 1);
        assert_eq!(first, vec![Segment { from: (0, 0), to: (300, 0), z: 64, cycles: 30 }]);
        assert_eq!(beam.take(), vec![Segment { from: (300, 0), to: (500, 0), z: 64, cycles: 20 }]);
    }

    #[test]
    fn lines_glow_on_the_phosphor_and_fade() {
        let mut screen = Screen::default();
        // Across the middle, from a quarter of the way in to three quarters.
        let line = Segment { from: (-HALF_X / 2, 0), to: (HALF_X / 2, 0), z: 127, cycles: 130 };
        screen.refresh(&[line]);
        let (mx, my) = to_pixel(0, 0);
        let (mx, my) = (mx as usize, my as usize);
        assert!(screen.light_at(mx, my) > 0.1, "lit along the line: {}", screen.light_at(mx, my));
        assert_eq!(screen.light_at(mx, my - 10), 0.0, "and dark away from it");
        assert_eq!(screen.light_at(WIDTH / 8, my), 0.0, "and past its end");
        let at_first = screen.light_at(mx, my);
        screen.refresh(&[]);
        assert!((screen.light_at(mx, my) - at_first * PERSIST).abs() < 1e-6, "fading");
        // The picture has the line bright, its glow beside it, and black away from it.
        let mut picture = Vec::new();
        screen.refresh(&[line]);
        screen.picture(&mut picture);
        let pixel = |x: usize, y: usize| &picture[(y * WIDTH + x) * 4..(y * WIDTH + x) * 4 + 4];
        assert!(pixel(mx, my)[2] > 200, "{:?}", pixel(mx, my));
        let near = pixel(mx, my - 3);
        assert!(near[2] > 0 && near[2] < pixel(mx, my)[2] && near[2] > near[0], "a blue glow beside it: {near:?}");
        assert_eq!(pixel(mx, my - 40), &[0, 0, 0, 255]);
        // A slow line is brighter than the same line drawn fast.
        let mut slow = Screen::default();
        slow.refresh(&[Segment { cycles: 400, ..line }]);
        assert!(slow.light_at(mx, my) > 2.0 * screen_after_one(line).light_at(mx, my));
    }

    fn screen_after_one(line: Segment) -> Screen {
        let mut screen = Screen::default();
        screen.refresh(&[line]);
        screen
    }
}
