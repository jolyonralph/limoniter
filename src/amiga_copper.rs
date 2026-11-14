//! A copper list run without an emulator, to see what it does: the beam goes down a PAL frame a colour clock at a
//! time, the copper waits for it and moves values into registers much as the hardware would (each instruction
//! taking four colour clocks), and the background, COLOR00, is drawn wherever the beam is when it changes, so raster bars,
//! splits part-way along a line and the like show as they would with no bitplanes over them. Every other colour
//! the list sets is kept line by line too. AGA colour writes follow BPLCON3's bank and LOCT.
//!
//! And copper gradients: colour stops on lines down the screen, made into a WAIT and a MOVE for each line the
//! colour changes on, in the original chips' 12-bit colour or AGA's 24-bit, past line 255 with a WAIT for $FFDF.

use crate::amiga_hw::Copper;

/// A PAL frame: 313 lines (the long one), each of 227 colour clocks ($00-$E2).
pub const LINES: usize = 313;
pub const CLOCKS: usize = 227;
/// How long the copper takes over an instruction, in colour clocks.
const STEP: usize = 4;

/// What a copper list did to a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    /// The background colour at each colour clock, line by line.
    pub pixels: Vec<[u8; 3]>,
    /// The colours the list set, other than COLOR00, by number (AGA: up to 255).
    pub touched: Vec<u16>,
    /// Each of those colours at the end of each line.
    pub palettes: Vec<Vec<[u8; 3]>>,
    /// Where the copper stopped: at the END, a jump, or a wait for a place the frame never reaches; None if it
    /// ran off the end of what it was given.
    pub stopped: Option<(usize, usize)>,
}

impl Preview {
    pub fn at(&self, line: usize, clock: usize) -> [u8; 3] {
        self.pixels[line * CLOCKS + clock]
    }
}

/// A 12-bit colour as 24-bit, each four bits doubled up.
fn wide(nibble: u16) -> u8 {
    (nibble as u8 & 15) * 17
}

struct Beam {
    pos: usize,
    painted: usize,
    palette: [[u8; 3]; 256],
    bplcon3: u16,
    touched: Vec<u16>,
    pixels: Vec<[u8; 3]>,
    palettes: Vec<Vec<[u8; 3]>>,
}

impl Beam {
    /// Draws the background up to a place, keeping the palette as each line ends.
    fn paint_to(&mut self, to: usize) {
        let to = to.min(LINES * CLOCKS);
        while self.painted < to {
            self.pixels[self.painted] = self.palette[0];
            self.painted += 1;
            if self.painted.is_multiple_of(CLOCKS) {
                self.palettes.push(self.palette.to_vec());
            }
        }
    }

    fn set_colour(&mut self, n: u16, value: u16) {
        let index = (self.bplcon3 >> 13) as usize * 32 + n as usize;
        let [r, g, b] = [(value >> 8) & 15, (value >> 4) & 15, value & 15];
        let colour = &mut self.palette[index];
        if self.bplcon3 & 0x200 != 0 {
            // LOCT: the low four bits of each.
            *colour = [(colour[0] & 0xF0) | r as u8, (colour[1] & 0xF0) | g as u8, (colour[2] & 0xF0) | b as u8];
        } else {
            *colour = [wide(r), wide(g), wide(b)];
        }
        if index != 0 && !self.touched.contains(&(index as u16)) {
            self.touched.push(index as u16);
        }
    }
}

/// Runs a copper list over a frame. An instruction whose value isn't known (None) is passed over.
pub fn simulate(ops: &[Option<Copper>]) -> Preview {
    let mut beam = Beam { pos: 0, painted: 0, palette: [[0; 3]; 256], bplcon3: 0x0C00, touched: Vec::new(), pixels: vec![[0; 3]; LINES * CLOCKS], palettes: Vec::new() };
    let end = LINES * CLOCKS;
    let mut stopped = None;
    let mut skip_next = false;
    for op in ops {
        if beam.pos >= end {
            break;
        }
        let Some(op) = *op else {
            beam.pos += STEP;
            continue;
        };
        if std::mem::take(&mut skip_next) {
            beam.pos += STEP;
            continue;
        }
        match op {
            Copper::End => {
                stopped = Some((beam.pos / CLOCKS, beam.pos % CLOCKS));
                break;
            }
            Copper::Move { register, value } => {
                beam.pos += STEP;
                beam.paint_to(beam.pos);
                match register {
                    0x106 => beam.bplcon3 = value,
                    0x180..0x1C0 => beam.set_colour((register - 0x180) / 2, value),
                    // A jump would start the list again, or go to another: the preview stops here.
                    0x088 | 0x08A => {
                        stopped = Some((beam.pos / CLOCKS, beam.pos % CLOCKS));
                        break;
                    }
                    _ => {}
                }
            }
            Copper::Wait { line, x, line_mask, x_mask, skip, .. } => {
                beam.pos += STEP;
                // The copper sees only the low eight bits of the line: past 255 a wait counts from 256.
                let here = beam.pos / CLOCKS;
                let target_line = (here & 0x100) | (line & line_mask) as usize;
                let target = target_line * CLOCKS + ((x & x_mask) as usize).min(CLOCKS - 1);
                if skip {
                    skip_next = beam.pos >= target;
                    continue;
                }
                if target >= end {
                    stopped = Some((target_line.min(LINES - 1), (x & x_mask) as usize));
                    beam.pos = end;
                    break;
                }
                // Woken, the copper takes a moment to go on: after a wait for $FFDF that takes it into line 256.
                if beam.pos < target {
                    beam.pos = target + STEP / 2;
                }
            }
        }
    }
    beam.paint_to(end);
    let mut touched = beam.touched;
    touched.sort();
    let palettes = beam.palettes.iter().map(|p| touched.iter().map(|n| p[*n as usize]).collect()).collect();
    Preview { pixels: beam.pixels, touched, palettes, stopped }
}

/// A colour stop of a gradient: a line, and the colour there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stop {
    pub line: u16,
    pub rgb: [u8; 3],
}

/// A gradient down the screen, for one colour register.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gradient {
    pub stops: Vec<Stop>,
    /// Which colour register, 0-31.
    pub register: u16,
    /// AGA's 24-bit colour, through BPLCON3's LOCT; otherwise 12-bit.
    pub aga: bool,
    /// What BPLCON3 is otherwise, for AGA (its usual $0C00).
    pub bplcon3: u16,
    /// The horizontal position each wait is for.
    pub x: u8,
    /// A change every so many lines.
    pub every: u16,
}

impl Default for Gradient {
    fn default() -> Self {
        Gradient {
            stops: vec![Stop { line: 0x2C, rgb: [0, 0, 0x40] }, Stop { line: 0x90, rgb: [0xF0, 0x80, 0x20] }, Stop { line: 0x12B, rgb: [0, 0, 0x40] }],
            register: 0,
            aga: false,
            bplcon3: 0x0C00,
            x: 0x06,
            every: 1,
        }
    }
}

/// The colour a gradient has on a line, between the stops either side.
pub fn colour_at(stops: &[Stop], line: u16) -> [u8; 3] {
    let mut sorted: Vec<Stop> = stops.to_vec();
    sorted.sort_by_key(|s| s.line);
    let Some(first) = sorted.first() else {
        return [0; 3];
    };
    if line <= first.line {
        return first.rgb;
    }
    for pair in sorted.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if line <= b.line {
            let span = (b.line - a.line).max(1) as i32;
            let t = (line - a.line) as i32;
            let mix = |i: usize| (a.rgb[i] as i32 + (b.rgb[i] as i32 - a.rgb[i] as i32) * t / span) as u8;
            return [mix(0), mix(1), mix(2)];
        }
    }
    sorted.last().map(|s| s.rgb).unwrap_or_default()
}

/// Four bits of a colour component, rounded.
fn nibble(v: u8) -> u16 {
    ((v as u16 * 15 + 127) / 255).min(15)
}

impl Gradient {
    fn lines(&self) -> (u16, u16) {
        let first = self.stops.iter().map(|s| s.line).min().unwrap_or(0).min(LINES as u16 - 1);
        let last = self.stops.iter().map(|s| s.line).max().unwrap_or(0).min(LINES as u16 - 1);
        (first, last)
    }

    /// The copper instructions: for each line the colour changes on, a WAIT and its MOVEs, with a WAIT for $FFDF
    /// before the first line past 255.
    pub fn ops(&self) -> Vec<Copper> {
        let mut ops = Vec::new();
        let (first, last) = self.lines();
        let register = 0x180 + (self.register.min(31) << 1);
        let mut previous: Option<(u16, u16)> = None;
        let mut wrapped = false;
        let mut line = first;
        while line <= last {
            let [r, g, b] = colour_at(&self.stops, line);
            let high = (nibble(r) << 8) | (nibble(g) << 4) | nibble(b);
            let low = if self.aga { (((r & 15) as u16) << 8) | (((g & 15) as u16) << 4) | (b & 15) as u16 } else { 0 };
            if previous != Some((high, low)) {
                if line > 255 && !wrapped {
                    ops.push(Copper::Wait { line: 0xFF, x: 0xDE, line_mask: 0xFF, x_mask: 0xFE, blitter: false, skip: false });
                    wrapped = true;
                }
                ops.push(Copper::Wait { line: (line & 0xFF) as u8, x: self.x & 0xFE, line_mask: 0xFF, x_mask: 0xFE, blitter: false, skip: false });
                // The 24-bit colour from its high bits ...
                let high = if self.aga { ((r as u16 >> 4) << 8) | ((g as u16 >> 4) << 4) | (b as u16 >> 4) } else { high };
                ops.push(Copper::Move { register, value: high });
                if self.aga {
                    // ... then its low bits, with LOCT on for the moment.
                    ops.push(Copper::Move { register: 0x106, value: self.bplcon3 | 0x200 });
                    ops.push(Copper::Move { register, value: low });
                    ops.push(Copper::Move { register: 0x106, value: self.bplcon3 & !0x200 });
                }
                previous = Some((high, low));
            }
            line += self.every.max(1);
        }
        ops
    }

    /// The gradient as source: a `dc.w` line for each instruction.
    pub fn source(&self) -> String {
        self.ops().into_iter().map(crate::amiga_hw::copper_line).collect::<Vec<_>>().join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(words: &[u16]) -> Preview {
        let ops: Vec<Option<Copper>> = words.as_chunks::<2>().0.iter().map(|[a, b]| Some(Copper::decode(*a, *b))).collect();
        simulate(&ops)
    }

    #[test]
    fn raster_bars_and_splits_show_where_the_beam_was() {
        let preview = run(&[0x0180, 0x0F00, 0x6407, 0xFFFE, 0x0180, 0x000F, 0x6461, 0xFFFE, 0x0180, 0x00F0, 0xFFDF, 0xFFFE, 0x0A07, 0xFFFE, 0x0182, 0x0FFF, 0x0180, 0x0FFF, 0xFFFF, 0xFFFE]);
        assert_eq!(preview.at(50, 100), [255, 0, 0], "red until line $64");
        assert_eq!(preview.at(0x64, 0x30), [0, 0, 255], "blue from x $06 of line $64");
        assert_eq!(preview.at(0x64, 0x70), [0, 255, 0], "green from x $60, a split part-way along");
        assert_eq!(preview.at(0x109, 100), [0, 255, 0]);
        assert_eq!(preview.at(0x10A, 100), [255, 255, 255], "line $0A after $FFDF is line 266");
        assert_eq!(preview.touched, [1]);
        assert_eq!(preview.palettes[0x10A][0], [255, 255, 255]);
        assert_eq!(preview.palettes[0x100][0], [0, 0, 0]);
        assert_eq!(preview.stopped.map(|s| s.0), Some(0x10A));
    }

    #[test]
    fn aga_colours_take_their_bank_and_low_bits() {
        let preview = run(&[0x0106, 0x0C00, 0x0180, 0x0123, 0x0106, 0x0E00, 0x0180, 0x0456, 0x0106, 0x2C00, 0x0182, 0x0FFF, 0xFFFF, 0xFFFE]);
        assert_eq!(preview.at(10, 10), [0x14, 0x25, 0x36], "high bits $123, low bits $456");
        assert_eq!(preview.touched, [33], "bank 1, colour 1");
    }

    #[test]
    fn a_wait_for_a_line_that_never_comes_stops_the_list() {
        let preview = run(&[0x0180, 0x0FFF, 0xFFFF, 0xFFFE, 0x0180, 0x0000]);
        assert_eq!(preview.at(312, 226), [255, 255, 255]);
        assert_eq!(run(&[0xFFDF, 0xFFFE, 0x7007, 0xFFFE, 0x0180, 0x0F00]).at(312, 0), [0, 0, 0], "line $170 is past the frame");
    }

    #[test]
    fn gradients_become_waits_and_moves() {
        let gradient = Gradient { stops: vec![Stop { line: 0x2C, rgb: [0, 0, 0] }, Stop { line: 0x2C + 15, rgb: [255, 255, 255] }], ..Gradient::default() };
        let ops = gradient.ops();
        assert_eq!(ops.len(), 32, "a wait and a move on each of 16 lines");
        assert_eq!(ops[0], Copper::Wait { line: 0x2C, x: 6, line_mask: 0xFF, x_mask: 0xFE, blitter: false, skip: false });
        assert_eq!(ops[1], Copper::Move { register: 0x180, value: 0x000 });
        assert_eq!(ops[31], Copper::Move { register: 0x180, value: 0xFFF });
        let source = gradient.source();
        assert!(source.starts_with("dc.w $2C07,$FFFE\t; wait for line $2C, x $06\ndc.w $0180,$0000\t; COLOR00"), "{source}");
        // Each line of the preview is the colour the gradient set.
        let preview = simulate(&ops.iter().map(|o| Some(*o)).collect::<Vec<_>>());
        assert_eq!(preview.at(0x2C + 15, 100), [255, 255, 255]);
    }

    #[test]
    fn gradients_past_line_255_wait_for_ffdf_once() {
        let gradient = Gradient { stops: vec![Stop { line: 250, rgb: [0, 0, 0] }, Stop { line: 270, rgb: [0, 255, 0] }], register: 3, ..Gradient::default() };
        let ops = gradient.ops();
        let wraps: Vec<usize> = ops.iter().enumerate().filter(|(_, o)| matches!(o, Copper::Wait { line: 0xFF, x: 0xDE, .. })).map(|(i, _)| i).collect();
        assert_eq!(wraps.len(), 1);
        assert!(matches!(ops[wraps[0] + 1], Copper::Wait { line, .. } if line < 15), "then lines count from 256");
        assert!(ops.iter().all(|o| !matches!(o, Copper::Move { register, .. } if *register != 0x186)), "COLOR03 only");
        let preview = simulate(&ops.iter().map(|o| Some(*o)).collect::<Vec<_>>());
        assert_eq!(preview.palettes[270].first(), Some(&[0, 255, 0]));
    }

    #[test]
    fn aga_gradients_set_the_low_bits_through_loct() {
        let gradient = Gradient { stops: vec![Stop { line: 0x40, rgb: [0x12, 0x34, 0x56] }, Stop { line: 0x40, rgb: [0x12, 0x34, 0x56] }], aga: true, ..Gradient::default() };
        let ops = gradient.ops();
        assert_eq!(&ops[1..], &[Copper::Move { register: 0x180, value: 0x135 }, Copper::Move { register: 0x106, value: 0x0E00 }, Copper::Move { register: 0x180, value: 0x246 }, Copper::Move { register: 0x106, value: 0x0C00 }]);
        let preview = simulate(&ops.iter().map(|o| Some(*o)).collect::<Vec<_>>());
        assert_eq!(preview.at(0x41, 100), [0x12, 0x34, 0x56]);
        assert_eq!(colour_at(&Gradient::default().stops, 0x2C), [0, 0, 0x40]);
    }
}
