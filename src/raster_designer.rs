//! The raster and copper designer: colour bars (a gradient down the lines they cover) and splits (from a line
//! on, a colour or a register changed) placed on a picture of a whole frame, Amiga, C64 or Atari ST, and made
//! into the code that does them: an Amiga copper list, a C64 chain of raster interrupts, or ST Timer B handlers.
//!
//! The design is worked out a line at a time into what each register should hold on each line, and only the
//! changes are written: a WAIT and MOVEs on the Amiga, a wait for $D012 and stores on the C64, a Timer B
//! interrupt on the ST. Colours are made the nearest each machine can show: 12-bit on the Amiga's original chips
//! and the STE, 24-bit on AGA, 9-bit on the ST, and the C64's 16.
//!
//! The Amiga's lines past 255 follow How To Code v8's chapter 6: one WAIT for $FFDF (the end of line 255) before
//! the first WAIT for a line after it, as a WAIT holds only the low 8 bits of the line. The C64's code is a chain
//! of raster interrupts made by the raster interrupt helper (`c64_irq`); each interrupt comes two lines before
//! its changes, and each change waits for $D012 to reach its line, so it lands in the first cycles of the line,
//! before the picture starts, bad line or not. The ST's is set up as the MFP timer calculator (`mfp_timer`) sets
//! Timer B up, counting display lines.

use crate::amiga_hw::Copper;
use crate::c64_raster::Vic;
use eframe::egui::{self, Color32, RichText, Sense, Ui, vec2};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// The machine designed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine {
    /// The Amiga's original chips (OCS and ECS): 12-bit colour.
    Amiga,
    /// AGA: 24-bit colour.
    AmigaAga,
    C64,
    /// The ST: 9-bit colour.
    St,
    /// The STE: 12-bit colour.
    Ste,
}

/// The C64's colour registers a bar or split can change, by their addresses and names.
const C64_COLOURS: [&str; 5] = ["$D020 border", "$D021 background", "$D022 background 1", "$D023 background 2", "$D024 background 3"];

impl Machine {
    pub const ALL: [Machine; 5] = [Machine::Amiga, Machine::AmigaAga, Machine::C64, Machine::St, Machine::Ste];

    pub fn name(self) -> &'static str {
        match self {
            Machine::Amiga => "Amiga (OCS, ECS)",
            Machine::AmigaAga => "Amiga (AGA)",
            Machine::C64 => "Commodore 64",
            Machine::St => "Atari ST",
            Machine::Ste => "Atari STE",
        }
    }

    pub fn is_amiga(self) -> bool {
        matches!(self, Machine::Amiga | Machine::AmigaAga)
    }

    pub fn is_st(self) -> bool {
        matches!(self, Machine::St | Machine::Ste)
    }

    /// The colour registers a bar or split can change: each one's number here, and its name.
    pub fn colours(self) -> Vec<(u16, String)> {
        match self {
            Machine::Amiga | Machine::AmigaAga => (0..32).map(|n| (n, format!("COLOR{n:02}"))).collect(),
            Machine::C64 => C64_COLOURS.iter().enumerate().map(|(n, name)| (n as u16, crate::i18n::tr(name).to_string())).collect(),
            Machine::St | Machine::Ste => (0..16).map(|n| (n, crate::i18n::tf!("colour {n} ($FF{:04X})", 0x8240 + n * 2))).collect(),
        }
    }

    /// The other registers a split can change: name, address (the Amiga's as an offset from $DFF000), and its
    /// usual value, which it has at the top of the frame unless the design says otherwise.
    pub fn registers(self) -> &'static [(&'static str, u32, u32)] {
        match self {
            Machine::Amiga | Machine::AmigaAga => &[
                ("BPLCON0 (bitplanes, resolution)", 0x100, 0x0200),
                ("BPLCON1 (horizontal scroll)", 0x102, 0x0000),
                ("BPLCON2 (priorities)", 0x104, 0x0024),
                ("BPL1MOD (odd planes' modulo)", 0x108, 0x0000),
                ("BPL2MOD (even planes' modulo)", 0x10A, 0x0000),
                ("DDFSTRT (fetch start)", 0x092, 0x0038),
                ("DDFSTOP (fetch stop)", 0x094, 0x00D0),
            ],
            Machine::C64 => &[
                ("$D011 (text or bitmap, vertical scroll)", 0xD011, 0x1B),
                ("$D016 (multicolour, horizontal scroll)", 0xD016, 0xC8),
                ("$D018 (screen and character memory)", 0xD018, 0x15),
                ("$D015 (sprites shown)", 0xD015, 0x00),
                ("$D01C (sprites in multicolour)", 0xD01C, 0x00),
            ],
            Machine::St => &[("$FF8260 (resolution: 0 low, 1 medium)", 0xFF8260, 0)],
            Machine::Ste => &[("$FF8260 (resolution: 0 low, 1 medium)", 0xFF8260, 0), ("$FF8265 (horizontal scroll)", 0xFF8265, 0), ("$FF820F (line offset)", 0xFF820F, 0)],
        }
    }
}

/// A colour bar: a gradient through its colours, spread evenly from its first line to its last, on one or more
/// colour registers. Below it, they're back to what they were.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bar {
    pub top: u16,
    pub height: u16,
    pub colours: Vec<[u8; 3]>,
    pub targets: Vec<u16>,
}

/// What a split changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Colour { index: u16, rgb: [u8; 3] },
    Register { address: u32, value: u32 },
    /// The Amiga's bitplane pointers, to a picture at a label: so many planes, so many bytes apart.
    Planes { count: u8, label: String, bytes: u32 },
}

/// From a line on, things changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Split {
    pub line: u16,
    pub changes: Vec<Change>,
}

/// What's chosen in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chosen {
    Bar(usize),
    Split(usize),
}

/// A register, as the design keeps track of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reg {
    Colour(u16),
    Register(u32),
}

/// One thing done on a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Write {
    /// A register set to a value as the hardware takes it (a colour in the machine's own form).
    Set(Reg, u32),
    /// The Amiga's bitplane pointers, filled in by code: the split they're from.
    Planes { split: usize, count: u8 },
}

/// The design.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Design {
    pub machine: Machine,
    /// NTSC (60 Hz on the ST) rather than PAL (50 Hz).
    pub ntsc: bool,
    /// The colours at the top of the frame, for each register that changes (black otherwise).
    pub base: BTreeMap<u16, [u8; 3]>,
    /// Other registers' values at the top of the frame, where they aren't the usual.
    pub base_registers: BTreeMap<u32, u32>,
    pub bars: Vec<Bar>,
    pub splits: Vec<Split>,
    /// What the labels start with.
    pub prefix: String,
    /// On the C64, whether the KERNAL stays in.
    pub kernal: bool,
}

/// The ST's first display line, at 50 and 60 Hz.
const ST_FIRST: [u16; 2] = [63, 34];

impl Design {
    /// A start: a bar and a split, for the machine.
    pub fn new(machine: Machine) -> Design {
        let (bar, split, after) = match machine {
            Machine::Amiga | Machine::AmigaAga => (0x60, 0xD0, [0x30, 0x10, 0x40]),
            Machine::C64 => (0x60, 0xC0, [0x00, 0x00, 0x00]),
            Machine::St | Machine::Ste => (40, 120, [0x20, 0x00, 0x40]),
        };
        let colours = vec![[0x10, 0x10, 0x80], [0xF0, 0xF0, 0xFF], [0x10, 0x10, 0x80]];
        // The C64's own colours after a reset: a light blue border round a blue background.
        let base = if machine == Machine::C64 { [(0, [0x6C, 0x5E, 0xB5]), (1, [0x35, 0x28, 0x79])].into_iter().collect() } else { BTreeMap::new() };
        Design {
            machine,
            ntsc: false,
            base,
            base_registers: BTreeMap::new(),
            bars: vec![Bar { top: bar, height: 24, colours, targets: vec![0] }],
            splits: vec![Split { line: split, changes: vec![Change::Colour { index: if machine == Machine::C64 { 1 } else { 0 }, rgb: after }] }],
            prefix: "raster".into(),
            kernal: false,
        }
    }

    fn vic(&self) -> Vic {
        if self.ntsc { Vic::Ntsc } else { Vic::Pal }
    }

    /// The lines in a frame.
    pub fn frame_lines(&self) -> u16 {
        match (self.machine, self.ntsc) {
            (Machine::C64, _) => self.vic().lines() as u16,
            (_, false) => 313,
            (_, true) => 263,
        }
    }

    /// The lines a bar or split can be on: on the ST, the display's lines from 1 (Timer B counts only those; line
    /// 0's colours are set at the top of the frame); elsewhere, every line shown.
    pub fn usable(&self, line: u16) -> bool {
        match self.machine {
            Machine::Amiga | Machine::AmigaAga => (0x1A..self.frame_lines()).contains(&line),
            Machine::C64 => u32::from(line) < self.vic().lines() && !self.vic().is_blank(u32::from(line)),
            Machine::St | Machine::Ste => (1..200).contains(&line),
        }
    }

    /// Where the frame starts, as a line: what comes before it is the end of the frame before.
    fn start(&self) -> u16 {
        match self.machine {
            Machine::C64 => self.vic().blank().0 as u16,
            _ => 0,
        }
    }

    /// A line's row in the picture of the frame, from its top.
    pub fn row_of(&self, line: u16) -> u16 {
        let lines = self.frame_lines();
        match self.machine {
            Machine::C64 => (line + lines - self.start()) % lines,
            Machine::St | Machine::Ste => ST_FIRST[usize::from(self.ntsc)] + line,
            _ => line,
        }
    }

    /// The line at a row of the picture.
    pub fn line_of_row(&self, row: u16) -> u16 {
        let lines = self.frame_lines();
        let row = row.min(lines - 1);
        match self.machine {
            Machine::C64 => (row + self.start()) % lines,
            Machine::St | Machine::Ste => row.saturating_sub(ST_FIRST[usize::from(self.ntsc)]).min(199),
            _ => row,
        }
    }

    /// Whether a row of the picture isn't shown (vertical blank, or off the top or bottom of an ST's screen).
    pub fn row_hidden(&self, row: u16) -> bool {
        match self.machine {
            Machine::Amiga | Machine::AmigaAga => row < 0x1A,
            Machine::C64 => self.vic().is_blank(u32::from(self.line_of_row(row))),
            // A monitor shows about 29 lines of border above and below the ST's 200 at 50 Hz, fewer at 60.
            Machine::St | Machine::Ste => {
                let first = ST_FIRST[usize::from(self.ntsc)];
                let border = if self.ntsc { 16 } else { 29 };
                row + border < first || row >= first + 200 + border
            }
        }
    }

    /// The rows of the display window (where the picture is, inside the border).
    pub fn window_rows(&self) -> (u16, u16) {
        match self.machine {
            Machine::Amiga | Machine::AmigaAga => (0x2C, if self.ntsc { 0xF3 } else { 0x12B }),
            Machine::C64 => {
                let (a, b) = crate::c64_raster::window(true);
                (self.row_of(a as u16), self.row_of(b as u16))
            }
            Machine::St | Machine::Ste => (self.row_of(0), self.row_of(199)),
        }
    }

    /// The usable lines in the order the beam comes to them.
    fn order(&self) -> Vec<u16> {
        (0..self.frame_lines()).map(|row| self.line_of_row(row)).filter(|l| self.usable(*l)).fold(Vec::new(), |mut v, l| {
            if v.last() != Some(&l) {
                v.push(l);
            }
            v
        })
    }

    /// A colour as the machine holds it.
    pub fn hardware(&self, rgb: [u8; 3]) -> u32 {
        let four = |v: u8| (u32::from(v) * 15 + 127) / 255;
        let three = |v: u8| (u32::from(v) * 7 + 127) / 255;
        match self.machine {
            Machine::Amiga => four(rgb[0]) << 8 | four(rgb[1]) << 4 | four(rgb[2]),
            Machine::AmigaAga => u32::from(rgb[0]) << 16 | u32::from(rgb[1]) << 8 | u32::from(rgb[2]),
            Machine::C64 => nearest_c64(rgb) as u32,
            Machine::St => three(rgb[0]) << 8 | three(rgb[1]) << 4 | three(rgb[2]),
            // The STE keeps each component's lowest bit at the top of its four, so an ST's colours mean the same.
            Machine::Ste => {
                let ste = |v: u8| {
                    let n = four(v);
                    (n >> 1) | (n & 1) << 3
                };
                ste(rgb[0]) << 8 | ste(rgb[1]) << 4 | ste(rgb[2])
            }
        }
    }

    /// The colour a value of the machine's shows.
    pub fn shown(&self, value: u32) -> [u8; 3] {
        let nibble = |shift: u32| (value >> shift & 15) as u8;
        match self.machine {
            Machine::Amiga => [nibble(8) * 17, nibble(4) * 17, nibble(0) * 17],
            Machine::AmigaAga => [(value >> 16) as u8, (value >> 8) as u8, value as u8],
            Machine::C64 => crate::c64_gfx::PEPTO[(value & 15) as usize],
            Machine::St => [nibble(8), nibble(4), nibble(0)].map(|n| (u16::from(n & 7) * 255 / 7) as u8),
            Machine::Ste => {
                let back = |n: u8| ((n & 7) << 1 | n >> 3) * 17;
                [back(nibble(8)), back(nibble(4)), back(nibble(0))]
            }
        }
    }

    /// A machine's colour value written for people.
    pub fn colour_words(&self, value: u32) -> String {
        match self.machine {
            Machine::AmigaAga => format!("${value:06X}"),
            Machine::C64 => format!("{value} ({})", crate::c64_hw::COLOUR_NAMES[(value & 15) as usize]),
            _ => format!("${value:03X}"),
        }
    }

    /// A bar's colour on a line, as the machine has it.
    pub fn bar_colour(&self, bar: &Bar, line: u16) -> u32 {
        let stops = &bar.colours;
        if stops.len() < 2 || bar.height < 2 {
            return self.hardware(stops.first().copied().unwrap_or_default());
        }
        // Where the line is along the bar, from 0 to the number of gaps between colours.
        let along = f64::from(line.saturating_sub(bar.top)) / f64::from(bar.height - 1) * (stops.len() - 1) as f64;
        let i = (along.floor() as usize).min(stops.len() - 2);
        let t = along - i as f64;
        let mix = |c: usize| (f64::from(stops[i][c]) + (f64::from(stops[i + 1][c]) - f64::from(stops[i][c])) * t).round().clamp(0.0, 255.0) as u8;
        self.hardware([mix(0), mix(1), mix(2)])
    }

    /// Whether a line is a bar's: its lines in the order the beam comes to them.
    fn in_bar(&self, bar: &Bar, line: u16) -> bool {
        let (row, top) = (self.row_of(line), self.row_of(bar.top));
        row >= top && row < top + bar.height
    }

    /// The registers the design changes, and what each holds at the top of the frame.
    pub fn top(&self) -> BTreeMap<Reg, u32> {
        let mut top = BTreeMap::new();
        let colour = |index: u16| self.hardware(self.base.get(&index).copied().unwrap_or_default());
        for bar in &self.bars {
            for t in &bar.targets {
                top.insert(Reg::Colour(*t), colour(*t));
            }
        }
        for split in &self.splits {
            for change in &split.changes {
                match change {
                    Change::Colour { index, .. } => {
                        top.insert(Reg::Colour(*index), colour(*index));
                    }
                    Change::Register { address, .. } => {
                        let usual = self.machine.registers().iter().find(|r| r.1 == *address).map_or(0, |r| r.2);
                        top.insert(Reg::Register(*address), self.base_registers.get(address).copied().unwrap_or(usual));
                    }
                    Change::Planes { .. } => {}
                }
            }
        }
        top
    }

    /// What's done on each line where something changes, in the order the beam comes to them: only what differs
    /// from the line before.
    pub fn writes(&self) -> Vec<(u16, Vec<Write>)> {
        let mut persistent = self.top();
        let mut current = persistent.clone();
        let mut out = Vec::new();
        for line in self.order() {
            let mut writes = Vec::new();
            for (index, split) in self.splits.iter().enumerate().filter(|(_, s)| s.line == line) {
                for change in &split.changes {
                    match change {
                        Change::Colour { index, rgb } => {
                            persistent.insert(Reg::Colour(*index), self.hardware(*rgb));
                        }
                        Change::Register { address, value } => {
                            persistent.insert(Reg::Register(*address), *value);
                        }
                        Change::Planes { count, .. } => writes.push(Write::Planes { split: index, count: *count }),
                    }
                }
            }
            let mut wanted = persistent.clone();
            for bar in self.bars.iter().filter(|b| self.in_bar(b, line)) {
                for t in &bar.targets {
                    wanted.insert(Reg::Colour(*t), self.bar_colour(bar, line));
                }
            }
            for (reg, value) in &wanted {
                if current.get(reg) != Some(value) {
                    writes.insert(writes.iter().take_while(|w| matches!(w, Write::Set(..))).count(), Write::Set(*reg, *value));
                }
            }
            current = wanted;
            if !writes.is_empty() {
                out.push((line, writes));
            }
        }
        out
    }

    /// What each register holds on each row of the picture, for drawing it.
    pub fn rows(&self) -> Vec<BTreeMap<Reg, u32>> {
        let mut state = self.top();
        let writes = self.writes();
        let mut next = writes.iter().peekable();
        let mut rows = Vec::new();
        for row in 0..self.frame_lines() {
            while let Some((l, w)) = next.peek() {
                if self.row_of(*l) > row {
                    break;
                }
                for write in w {
                    if let Write::Set(reg, value) = write {
                        state.insert(*reg, *value);
                    }
                }
                next.next();
            }
            rows.push(state.clone());
        }
        rows
    }

    /// Why the design can't be made, if it can't.
    pub fn problem(&self) -> Option<String> {
        let p = self.prefix.trim();
        if p.is_empty() || !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || p.starts_with(|c: char| c.is_ascii_digit()) {
            return Some(crate::i18n::tr("The labels' prefix has to be a word of letters, digits and _.").into());
        }
        for bar in &self.bars {
            if bar.height == 0 || bar.colours.is_empty() || bar.targets.is_empty() {
                return Some(crate::i18n::tf!("The bar at line {} needs a height, a colour and a register to change.", bar.top));
            }
        }
        for split in &self.splits {
            for change in &split.changes {
                if let Change::Planes { label, .. } = change
                    && (label.trim().is_empty() || !label.trim().chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.'))
                {
                    return Some(crate::i18n::tf!("The split at line {} needs the label of the picture its bitplanes are in.", split.line));
                }
            }
        }
        None
    }

    /// The source, as the machine's code: a copper list, raster interrupts or Timer B handlers.
    pub fn source(&self) -> Result<String, String> {
        if let Some(why) = self.problem() {
            return Err(why);
        }
        match self.machine {
            Machine::Amiga | Machine::AmigaAga => Ok(self.amiga_source()),
            Machine::C64 => self.c64_source(),
            Machine::St | Machine::Ste => Ok(self.st_source()),
        }
    }

    /// The copper instructions, and a comment for any that needs one more than its own.
    pub fn copper(&self) -> Vec<(Copper, Option<String>)> {
        let aga = self.machine == Machine::AmigaAga;
        let mut ops: Vec<(Copper, Option<String>)> = Vec::new();
        let moves = |ops: &mut Vec<(Copper, Option<String>)>, writes: &[Write]| {
            for write in writes {
                match write {
                    Write::Set(Reg::Colour(n), value) => {
                        let register = 0x180 + (n.min(&31) << 1);
                        if aga {
                            // The high four bits of each component, then the low four with LOCT on for the moment.
                            let high = |shift: u32| (value >> (shift + 4) & 15) as u16;
                            let low = |shift: u32| (value >> shift & 15) as u16;
                            ops.push((Copper::Move { register, value: high(16) << 8 | high(8) << 4 | high(0) }, None));
                            ops.push((Copper::Move { register: 0x106, value: 0x0E00 }, Some(crate::i18n::tr("BPLCON3: LOCT on, for the low bits").into())));
                            ops.push((Copper::Move { register, value: low(16) << 8 | low(8) << 4 | low(0) }, None));
                            ops.push((Copper::Move { register: 0x106, value: 0x0C00 }, Some(crate::i18n::tr("BPLCON3: LOCT off again").into())));
                        } else {
                            ops.push((Copper::Move { register, value: *value as u16 }, None));
                        }
                    }
                    Write::Set(Reg::Register(address), value) => ops.push((Copper::Move { register: *address as u16 & 0x1FE, value: *value as u16 }, None)),
                    Write::Planes { split, count } => {
                        for plane in 0..u16::from(*count) {
                            let said = (plane == 0).then(|| crate::i18n::tf!("bitplane pointers, filled in by {}_set_planes: split {}", self.prefix.trim(), split + 1));
                            ops.push((Copper::Move { register: 0xE0 + plane * 4, value: 0 }, said));
                            ops.push((Copper::Move { register: 0xE2 + plane * 4, value: 0 }, None));
                        }
                    }
                }
            }
        };
        let top: Vec<Write> = self.top().into_iter().map(|(reg, value)| Write::Set(reg, value)).collect();
        moves(&mut ops, &top);
        let mut wrapped = false;
        for (line, writes) in self.writes() {
            if line > 255 && !wrapped {
                ops.push((Copper::Wait { line: 0xFF, x: 0xDE, line_mask: 0xFF, x_mask: 0xFE, blitter: false, skip: false }, Some(crate::i18n::tr("wait for the end of line 255: the next WAIT's line counts from 256").into())));
                wrapped = true;
            }
            ops.push((Copper::Wait { line: (line & 0xFF) as u8, x: 0x06, line_mask: 0xFF, x_mask: 0xFE, blitter: false, skip: false }, (line > 255).then(|| crate::i18n::tf!("wait for line ${line:03X}, x $06"))));
            moves(&mut ops, &writes);
        }
        ops.push((Copper::End, None));
        ops
    }

    /// The copper list's lines, without its label.
    pub fn copper_lines(&self) -> Vec<String> {
        let mut planes = 0;
        let mut out = Vec::new();
        for (op, said) in self.copper() {
            if said.is_some() && matches!(op, Copper::Move { register: 0xE0, .. }) {
                planes += 1;
                out.push(format!("{}_planes{planes}:", self.prefix.trim()));
            }
            let line = crate::amiga_hw::copper_line(op);
            let (code, note) = line.split_once('\t').unwrap_or((&line, ""));
            let code = code.replacen("dc.w ", "dc.w    ", 1);
            let note = said.unwrap_or_else(|| note.trim_start_matches("; ").to_string());
            out.push(format!("        {code:<32}; {note}"));
        }
        out
    }

    fn amiga_source(&self) -> String {
        let p = self.prefix.trim();
        let mut out = String::new();
        let _ = writeln!(
            out,
            "; A copper list from Limoniter's raster designer: {}, {}.\n; It belongs in chip memory (a data_c section). Point COP1LC at {p}_copper.",
            if self.ntsc { "NTSC" } else { "PAL" },
            if self.machine == Machine::AmigaAga { "AGA's 24-bit colour" } else { "the original chips' 12-bit colour" }
        );
        let _ = writeln!(out, "{p}_copper:");
        for line in self.copper_lines() {
            let _ = writeln!(out, "{line}");
        }
        let planes: Vec<(usize, &Split)> = self.splits.iter().enumerate().filter(|(_, s)| s.changes.iter().any(|c| matches!(c, Change::Planes { .. }))).collect();
        if !planes.is_empty() {
            let _ = writeln!(out, "\n; Puts the pictures' addresses into the list's bitplane pointers: call it once, from your code section,\n; before the list is shown.");
            let _ = writeln!(out, "{p}_set_planes:");
            let mut n = 0;
            for (_, split) in planes {
                for change in &split.changes {
                    if let Change::Planes { count, label, bytes } = change {
                        n += 1;
                        let _ = writeln!(out, "        move.l  #{},d0", label.trim());
                        let _ = writeln!(out, "        lea     {p}_planes{n}+2,a0");
                        let _ = writeln!(out, "        moveq   #{},d1", count.max(&1) - 1);
                        let _ = writeln!(out, ".plane{n}:");
                        let _ = writeln!(out, "        swap    d0");
                        let _ = writeln!(out, "        move.w  d0,(a0)                 ; the high word");
                        let _ = writeln!(out, "        swap    d0");
                        let _ = writeln!(out, "        move.w  d0,4(a0)                ; then the low word");
                        let _ = writeln!(out, "        add.l   #{bytes},d0               ; the next plane");
                        let _ = writeln!(out, "        addq.l  #8,a0");
                        let _ = writeln!(out, "        dbf     d1,.plane{n}");
                    }
                }
            }
            let _ = writeln!(out, "        rts");
        }
        out
    }

    /// A whole Amiga program showing the list, made from the "copper only" starter: to try it at once.
    pub fn amiga_program(&self) -> String {
        let starter = crate::amiga_starters::COPPER;
        let mut out = String::new();
        let mut skipping = false;
        for line in starter.lines() {
            if line.contains("; Each line: wait for it") {
                skipping = true;
                let _ = writeln!(out, "        ; From Limoniter's raster designer.");
                for l in self.copper_lines() {
                    let _ = writeln!(out, "{l}");
                }
                continue;
            }
            if skipping {
                if line.contains("$ffff,$fffe") {
                    skipping = false;
                }
                continue;
            }
            // The bitplane pointers are filled in before the list is shown.
            if line.trim_start().starts_with("; Our copper list from the top of a frame") && self.splits.iter().any(|s| s.changes.iter().any(|c| matches!(c, Change::Planes { .. }))) {
                let _ = writeln!(out, "        bsr     {}_set_planes", self.prefix.trim());
            }
            let _ = writeln!(out, "{line}");
            if line.trim() == "even" {
                let source = self.amiga_source();
                if let Some(at) = source.find("\n; Puts the pictures'") {
                    let _ = writeln!(out, "{}", &source[at..]);
                }
            }
        }
        out
    }

    /// The C64's lines with changes, gathered into runs a raster interrupt each: changes less than three lines
    /// apart share one, as each interrupt comes two lines before its first change.
    fn c64_runs(&self) -> Vec<Vec<(u16, Vec<Write>)>> {
        let mut runs: Vec<Vec<(u16, Vec<Write>)>> = Vec::new();
        for (line, writes) in self.writes() {
            match runs.last_mut() {
                Some(run) if self.row_of(line) <= self.row_of(run.last().expect("a run has a line").0) + 3 => run.push((line, writes)),
                _ => runs.push(vec![(line, writes)]),
            }
        }
        runs
    }

    fn c64_address(reg: Reg) -> u32 {
        match reg {
            Reg::Colour(n) => 0xD020 + u32::from(n),
            Reg::Register(a) => a,
        }
    }

    fn c64_source(&self) -> Result<String, String> {
        let p = self.prefix.trim();
        let lines = self.frame_lines();
        let runs = self.c64_runs();
        let top_line = self.start();
        let mut entries = vec![crate::c64_irq::Entry { line: u32::from(top_line), routine: format!("{p}_top"), stable: false }];
        for (i, run) in runs.iter().enumerate() {
            let first = run[0].0;
            entries.push(crate::c64_irq::Entry { line: u32::from((first + lines - 2) % lines), routine: format!("{p}_run{}", i + 1), stable: false });
        }
        let chain = crate::c64_irq::Chain { vic: self.vic(), kernal: self.kernal, prefix: p.to_string(), entries };
        let mut out = String::new();
        let _ = writeln!(
            out,
            "; Raster colours from Limoniter's raster designer, for the C64 ({}). Each interrupt comes two lines before\n; its changes; each change waits for $D012 to reach its line, then stores, so it lands in the first cycles\n; of the line, before the picture starts, bad line or not.",
            self.vic().short()
        );
        out.push_str(&chain.source()?);
        let line = |out: &mut String, code: &str, comment: &str| {
            let _ = if comment.is_empty() { writeln!(out, "        {code}") } else { writeln!(out, "        {code:<24}; {comment}") };
        };
        let _ = writeln!(out, "; The top of the frame: everything as it is above the first change.");
        let _ = writeln!(out, "{p}_top");
        for (reg, value) in self.top() {
            line(&mut out, &format!("lda #${value:02x}"), "");
            line(&mut out, &format!("sta ${:04x}", Self::c64_address(reg)), "");
        }
        line(&mut out, "rts", "");
        for (i, run) in runs.iter().enumerate() {
            let _ = writeln!(out, "\n; Lines {} to {}.", run[0].0, run[run.len() - 1].0);
            let _ = writeln!(out, "{p}_run{}", i + 1);
            for (l, writes) in run {
                let sets: Vec<(u32, u32)> = writes.iter().filter_map(|w| if let Write::Set(reg, value) = w { Some((Self::c64_address(*reg), *value)) } else { None }).collect();
                line(&mut out, &format!("lda #${:02x}", l & 0xFF), &crate::i18n::tf!("line {l}"));
                // The first two values wait in X and Y, so their stores come straight after the wait.
                let held = ["x", "y"];
                for (n, (_, value)) in sets.iter().take(2).enumerate() {
                    line(&mut out, &format!("ld{} #${value:02x}", held[n]), "");
                }
                line(&mut out, "cmp $d012", "");
                line(&mut out, "bne *-3", crate::i18n::tr("until the line starts"));
                for (n, (address, value)) in sets.iter().enumerate() {
                    if n < 2 {
                        line(&mut out, &format!("st{} ${address:04x}", held[n]), "");
                    } else {
                        line(&mut out, &format!("lda #${value:02x}"), "");
                        line(&mut out, &format!("sta ${address:04x}"), "");
                    }
                }
            }
            line(&mut out, "rts", "");
        }
        Ok(out)
    }

    fn st_address(reg: Reg) -> (u32, bool) {
        match reg {
            Reg::Colour(n) => (0xFF8240 + u32::from(n) * 2, true),
            Reg::Register(a) => (a, false),
        }
    }

    fn st_source(&self) -> String {
        let p = self.prefix.trim();
        let writes = self.writes();
        let short = |address: u32| format!("$ffff{:04x}.w", address & 0xFFFF);
        let line = |out: &mut String, code: &str, comment: &str| {
            let _ = if comment.is_empty() { writeln!(out, "        {code}") } else { writeln!(out, "        {code:<32}; {comment}") };
        };
        let set = |out: &mut String, reg: Reg, value: u32| {
            let (address, word) = Self::st_address(reg);
            if word {
                line(out, &format!("move.w  #${value:03x},{}", short(address)), "");
            } else {
                line(out, &format!("move.b  #${value:02x},{}", short(address)), "");
            }
        };
        let restart = |out: &mut String, count: u16, label: &str| {
            line(out, &format!("move.l  #{label},$120.w"), crate::i18n::tr("the next line's handler"));
            line(out, "clr.b   $fffffa1b.w", crate::i18n::tr("Timer B stopped while it's set"));
            line(out, &format!("move.b  #{count},$fffffa21.w"), &crate::i18n::tf!("{count} lines on"));
            line(out, "move.b  #8,$fffffa1b.w", crate::i18n::tr("counting display lines again"));
        };
        let mut out = String::new();
        let _ = writeln!(
            out,
            "; Raster colours from Limoniter's raster designer, for the {} ({} Hz). Timer B counts display lines: it\n; interrupts at the end of the line before each change, so the change shows from the start of its line.\n; Call {p}_setup once (in supervisor mode), and {p}_vbl from your VBL handler every frame.",
            if self.machine == Machine::Ste { "STE" } else { "ST" },
            if self.ntsc { 60 } else { 50 }
        );
        let label = |n: usize| format!("{p}_line{n}");
        let _ = writeln!(out, "{p}_setup:");
        let first = writes.first().map_or(1, |w| w.0);
        out.push_str(&crate::mfp_timer::source(crate::mfp_timer::Timer::B, if writes.is_empty() { 0 } else { 8 }, first as u8, &label(1), "event count mode: one a display line"));
        line(&mut out, "rts", "");
        let _ = writeln!(out, "\n; Every frame, from the VBL: the top of the frame as it is above the first change, and Timer B started again.");
        let _ = writeln!(out, "{p}_vbl:");
        for (reg, value) in self.top() {
            set(&mut out, reg, value);
        }
        if !writes.is_empty() {
            restart(&mut out, first, &label(1));
        }
        line(&mut out, "rts", "");
        for (i, (l, w)) in writes.iter().enumerate() {
            let _ = writeln!(out, "\n; Display line {l}.");
            let _ = writeln!(out, "{}:", label(i + 1));
            for write in w {
                if let Write::Set(reg, value) = write {
                    set(&mut out, *reg, *value);
                }
            }
            match writes.get(i + 1) {
                Some((next, _)) => restart(&mut out, next - l, &label(i + 2)),
                None => line(&mut out, "clr.b   $fffffa1b.w", crate::i18n::tr("Timer B stopped until the next frame")),
            }
            line(&mut out, "bclr    #0,$fffffa0f.w", crate::i18n::tr("Timer B's interrupt is done"));
            line(&mut out, "rte", "");
        }
        out
    }
}

/// The nearest of the C64's colours.
pub fn nearest_c64(rgb: [u8; 3]) -> usize {
    let distance = |c: &[u8; 3]| (0..3).map(|i| (i32::from(c[i]) - i32::from(rgb[i])).pow(2)).sum::<i32>();
    crate::c64_gfx::PEPTO.iter().enumerate().min_by_key(|(_, c)| distance(c)).map_or(0, |(i, _)| i)
}

/// What the window asks of the editor.
pub enum Asked {
    Insert(String),
    /// A whole program to try it, as a new file.
    Program(String),
}

/// The designer's window.
pub struct Designer {
    pub design: Design,
    pub chosen: Option<Chosen>,
    /// What's being dragged: the item, where the drag started, and its line then.
    drag: Option<(Chosen, f32, u16)>,
}

impl Default for Designer {
    fn default() -> Self {
        Designer { design: Design::new(Machine::Amiga), chosen: Some(Chosen::Bar(0)), drag: None }
    }
}

fn colour32(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

impl Designer {
    pub fn for_machine(machine: Machine) -> Designer {
        Designer { design: Design::new(machine), ..Default::default() }
    }

    /// The item at a row of the picture: a split near it, or else a bar over it.
    fn item_at(&self, row: u16) -> Option<Chosen> {
        let d = &self.design;
        let split = d.splits.iter().enumerate().filter(|(_, s)| (i32::from(d.row_of(s.line)) - i32::from(row)).abs() <= 3).min_by_key(|(_, s)| (i32::from(d.row_of(s.line)) - i32::from(row)).abs());
        if let Some((i, _)) = split {
            return Some(Chosen::Split(i));
        }
        d.bars.iter().rposition(|b| row >= d.row_of(b.top) && row < d.row_of(b.top) + b.height).map(Chosen::Bar)
    }

    fn line_of(&self, chosen: Chosen) -> Option<u16> {
        match chosen {
            Chosen::Bar(i) => self.design.bars.get(i).map(|b| b.top),
            Chosen::Split(i) => self.design.splits.get(i).map(|s| s.line),
        }
    }

    fn set_line(&mut self, chosen: Chosen, line: u16) {
        let d = &mut self.design;
        match chosen {
            Chosen::Bar(i) => {
                if let Some(b) = d.bars.get_mut(i) {
                    b.top = line;
                }
            }
            Chosen::Split(i) => {
                if let Some(s) = d.splits.get_mut(i) {
                    s.line = line;
                }
            }
        }
    }

    /// The picture of the frame: the background colour on every line, the border on the C64, the other colours
    /// changed in strips beside it, each bar and split marked; they're dragged up and down.
    fn picture(&mut self, ui: &mut Ui) {
        let d = &self.design;
        let rows = d.frame_lines();
        let others: Vec<u16> = d.top().keys().filter_map(|r| if let Reg::Colour(n) = r { Some(*n) } else { None }).filter(|n| *n != if d.machine == Machine::C64 { 1 } else { 0 } && !(d.machine == Machine::C64 && *n == 0)).collect();
        let width = 300.0;
        let strip = 14.0;
        let (response, painter) = ui.allocate_painter(vec2(width + 6.0 + strip * others.len() as f32 + 110.0, f32::from(rows)), Sense::click_and_drag());
        let origin = response.rect.min;
        let state = d.rows();
        // The Amiga's copper run as the hardware would: the background where the beam is when it changes.
        let preview = d.machine.is_amiga().then(|| crate::amiga_copper::simulate(&d.copper().into_iter().map(|(op, _)| Some(op)).collect::<Vec<_>>()));
        let (window_top, window_bottom) = d.window_rows();
        for row in 0..rows {
            let y = origin.y + f32::from(row);
            let at = |x0: f32, x1: f32| egui::Rect::from_min_max(egui::pos2(origin.x + x0, y), egui::pos2(origin.x + x1, y + 1.0));
            let s = &state[usize::from(row)];
            let shown = |n: u16| d.shown(s.get(&Reg::Colour(n)).copied().unwrap_or_else(|| d.hardware(d.base.get(&n).copied().unwrap_or_default())));
            match (&preview, d.machine) {
                (Some(p), _) if usize::from(row) < crate::amiga_copper::LINES => {
                    // Colour clocks $00 to $E2 across; the picture shows from $20, where the left border starts.
                    for x in 0..60 {
                        let clock = 0x20 + x * 3;
                        painter.rect_filled(at(x as f32 * width / 60.0, (x + 1) as f32 * width / 60.0), 0.0, colour32(p.at(usize::from(row), clock)));
                    }
                }
                (_, Machine::C64) => {
                    painter.rect_filled(at(0.0, width), 0.0, colour32(shown(0)));
                    if (window_top..=window_bottom).contains(&row) {
                        painter.rect_filled(at(width * 0.1, width * 0.9), 0.0, colour32(shown(1)));
                    }
                }
                _ => {
                    painter.rect_filled(at(0.0, width), 0.0, colour32(shown(0)));
                }
            }
            for (k, n) in others.iter().enumerate() {
                let x = width + 6.0 + k as f32 * strip;
                painter.rect_filled(at(x, x + strip - 2.0), 0.0, colour32(shown(*n)));
            }
            if d.row_hidden(row) {
                painter.rect_filled(at(0.0, width), 0.0, Color32::from_black_alpha(170));
            }
        }
        // The display window's edges.
        let faint = egui::Stroke::new(1.0, Color32::from_white_alpha(60));
        for row in [window_top, window_bottom + 1] {
            let y = origin.y + f32::from(row);
            painter.hline(origin.x..=origin.x + width, y, faint);
        }
        // Line 256 on the Amiga, where a WAIT runs out of bits.
        if d.machine.is_amiga() {
            let y = origin.y + 256.0;
            painter.hline(origin.x + width - 20.0..=origin.x + width, y, egui::Stroke::new(1.0, Color32::from_rgb(255, 200, 0)));
        }
        let accent = crate::theme::accent();
        let font = egui::FontId::proportional(10.0);
        for (i, bar) in d.bars.iter().enumerate() {
            let top = origin.y + f32::from(d.row_of(bar.top));
            let rect = egui::Rect::from_min_size(egui::pos2(origin.x, top), vec2(width, f32::from(bar.height)));
            let chosen = self.chosen == Some(Chosen::Bar(i));
            painter.rect_stroke(rect, 0.0, egui::Stroke::new(if chosen { 2.0 } else { 1.0 }, if chosen { accent } else { Color32::from_white_alpha(90) }), egui::StrokeKind::Outside);
            painter.text(egui::pos2(origin.x + width + 6.0 + strip * others.len() as f32, top), egui::Align2::LEFT_TOP, line_words(d, bar.top), font.clone(), ui.visuals().text_color());
        }
        for (i, split) in d.splits.iter().enumerate() {
            let y = origin.y + f32::from(d.row_of(split.line));
            let chosen = self.chosen == Some(Chosen::Split(i));
            painter.hline(origin.x..=origin.x + width, y, egui::Stroke::new(if chosen { 2.0 } else { 1.0 }, if chosen { accent } else { Color32::WHITE }));
            painter.text(egui::pos2(origin.x + width + 6.0 + strip * others.len() as f32, y), egui::Align2::LEFT_CENTER, line_words(d, split.line), font.clone(), ui.visuals().text_color());
        }
        for (k, n) in others.iter().enumerate() {
            let x = origin.x + width + 6.0 + k as f32 * strip;
            painter.text(egui::pos2(x, origin.y), egui::Align2::LEFT_TOP, format!("{n}"), font.clone(), Color32::WHITE);
        }
        // Choosing and dragging.
        let row_at = |y: f32| ((y - origin.y).max(0.0) as u16).min(rows - 1);
        if let Some(pos) = response.hover_pos() {
            let line = d.line_of_row(row_at(pos.y));
            response.clone().on_hover_text(line_words(d, line));
        }
        if response.drag_started()
            && let Some(pos) = response.interact_pointer_pos()
            && let Some(item) = self.item_at(row_at(pos.y))
        {
            self.chosen = Some(item);
            self.drag = self.line_of(item).map(|line| (item, pos.y, line));
        }
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.chosen = self.item_at(row_at(pos.y)).or(self.chosen);
        }
        if let Some((item, from, line)) = self.drag {
            if response.dragged()
                && let Some(pos) = response.interact_pointer_pos()
            {
                let row = (i32::from(self.design.row_of(line)) + (pos.y - from).round() as i32).clamp(0, i32::from(rows) - 1) as u16;
                let new = self.design.line_of_row(row);
                if self.design.usable(new) {
                    self.set_line(item, new);
                }
            }
            if !response.dragged() {
                self.drag = None;
            }
        }
    }

    /// A colour chooser, with what the machine makes of it.
    fn colour(ui: &mut Ui, design: &Design, rgb: &mut [u8; 3]) {
        ui.color_edit_button_srgb(rgb);
        let value = design.hardware(*rgb);
        ui.label(RichText::new(design.colour_words(value)).monospace().small());
    }

    fn colour_target(ui: &mut Ui, id: impl egui::AsIdSalt, machine: Machine, index: &mut u16) {
        let colours = machine.colours();
        let shown = colours.iter().find(|c| c.0 == *index).map_or(String::new(), |c| c.1.clone());
        egui::ComboBox::from_id_salt(id).selected_text(shown).show_ui(ui, |ui| {
            for (n, name) in &colours {
                ui.selectable_value(index, *n, name.as_str());
            }
        });
    }

    fn line_edit(ui: &mut Ui, design: &Design, line: &mut u16) {
        let (low, high) = match design.machine {
            Machine::St | Machine::Ste => (1, 199),
            _ => (0, design.frame_lines() - 1),
        };
        ui.add(egui::DragValue::new(line).range(low..=high));
        ui.label(RichText::new(line_words(design, *line)).small());
    }

    /// The chosen bar's or split's settings.
    fn settings(&mut self, ui: &mut Ui) {
        let machine = self.design.machine;
        let snapshot = self.design.clone();
        match self.chosen {
            Some(Chosen::Bar(i)) if i < self.design.bars.len() => {
                let bar = &mut self.design.bars[i];
                egui::Grid::new("raster-bar").num_columns(2).show(ui, |ui| {
                    ui.label(crate::i18n::tr("First line"));
                    ui.horizontal(|ui| Self::line_edit(ui, &snapshot, &mut bar.top));
                    ui.end_row();
                    ui.label(crate::i18n::tr("Lines"));
                    ui.add(egui::DragValue::new(&mut bar.height).range(1..=200));
                    ui.end_row();
                    ui.label(crate::i18n::tr("Changes"));
                    ui.vertical(|ui| {
                        let mut gone = None;
                        for (k, target) in bar.targets.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                Self::colour_target(ui, ("bar-target", i, k), machine, target);
                                if bar_targets_removable(k) && crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Remove").clicked() {
                                    gone = Some(k);
                                }
                            });
                        }
                        if let Some(k) = gone {
                            bar.targets.remove(k);
                        }
                        if ui.small_button(crate::i18n::tr("And another register")).clicked() {
                            let next = machine.colours().iter().map(|c| c.0).find(|n| !bar.targets.contains(n)).unwrap_or(0);
                            bar.targets.push(next);
                        }
                    });
                    ui.end_row();
                    ui.label(crate::i18n::tr("Colours, top to bottom"));
                    ui.vertical(|ui| {
                        let mut gone = None;
                        for (k, rgb) in bar.colours.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                Self::colour(ui, &snapshot, rgb);
                                if k > 0 && crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Remove").clicked() {
                                    gone = Some(k);
                                }
                            });
                        }
                        if let Some(k) = gone {
                            bar.colours.remove(k);
                        }
                        if ui.small_button(crate::i18n::tr("Add a colour")).clicked() {
                            let last = bar.colours.last().copied().unwrap_or_default();
                            bar.colours.push(last);
                        }
                    });
                    ui.end_row();
                });
            }
            Some(Chosen::Split(i)) if i < self.design.splits.len() => {
                let split = &mut self.design.splits[i];
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Line"));
                    Self::line_edit(ui, &snapshot, &mut split.line);
                });
                let mut gone = None;
                for (k, change) in split.changes.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        match change {
                            Change::Colour { index, rgb } => {
                                Self::colour_target(ui, ("split-colour", i, k), machine, index);
                                Self::colour(ui, &snapshot, rgb);
                            }
                            Change::Register { address, value } => {
                                let registers = machine.registers();
                                let shown = registers.iter().find(|r| r.1 == *address).map_or(String::new(), |r| crate::i18n::tr(r.0).to_string());
                                egui::ComboBox::from_id_salt(("split-register", i, k)).selected_text(shown).show_ui(ui, |ui| {
                                    for r in registers {
                                        ui.selectable_value(address, r.1, crate::i18n::tr(r.0));
                                    }
                                });
                                let most = if machine.is_amiga() { 0xFFFF } else { 0xFF };
                                ui.add(egui::DragValue::new(value).range(0..=most).hexadecimal(if most > 0xFF { 4 } else { 2 }, false, true).prefix("$"));
                            }
                            Change::Planes { count, label, bytes } => {
                                ui.label(crate::i18n::tr("Bitplanes"));
                                ui.add(egui::DragValue::new(count).range(1..=8));
                                ui.label(crate::i18n::tr("at"));
                                ui.add(egui::TextEdit::singleline(label).desired_width(90.0).hint_text(crate::i18n::tr("label")));
                                ui.label(crate::i18n::tr("bytes apart"));
                                ui.add(egui::DragValue::new(bytes).range(2..=0xFFFFF));
                            }
                        }
                        if crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Remove").clicked() {
                            gone = Some(k);
                        }
                    });
                }
                if let Some(k) = gone {
                    split.changes.remove(k);
                }
                ui.horizontal(|ui| {
                    if ui.small_button(crate::i18n::tr("Change a colour")).clicked() {
                        split.changes.push(Change::Colour { index: if machine == Machine::C64 { 1 } else { 0 }, rgb: [0x80, 0x80, 0x80] });
                    }
                    if ui.small_button(crate::i18n::tr("Change a register")).clicked() {
                        let first = machine.registers()[0];
                        split.changes.push(Change::Register { address: first.1, value: first.2 });
                    }
                    if machine.is_amiga() && ui.small_button(crate::i18n::tr("Point the bitplanes elsewhere")).clicked() {
                        split.changes.push(Change::Planes { count: 1, label: "picture2".into(), bytes: 40 * 256 });
                    }
                });
            }
            _ => {
                ui.label(RichText::new(crate::i18n::tr("Choose a bar or split on the picture, or add one.")).color(ui.visuals().weak_text_color()));
            }
        }
    }

    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        let d = &mut self.design;
        ui.horizontal(|ui| {
            let mut machine = d.machine;
            egui::ComboBox::from_id_salt("raster-machine").selected_text(crate::i18n::tr(machine.name())).show_ui(ui, |ui| {
                for m in Machine::ALL {
                    ui.selectable_value(&mut machine, m, crate::i18n::tr(m.name()));
                }
            });
            if machine != d.machine {
                // The same family keeps the design; another machine starts again, its registers being different.
                if machine.is_amiga() == d.machine.is_amiga() && machine.is_st() == d.machine.is_st() {
                    d.machine = machine;
                } else {
                    let (ntsc, prefix) = (d.ntsc, d.prefix.clone());
                    *d = Design { ntsc, prefix, ..Design::new(machine) };
                    self.chosen = Some(Chosen::Bar(0));
                }
            }
            let (pal, ntsc) = if d.machine.is_st() { ("50 Hz", "60 Hz") } else { ("PAL", "NTSC") };
            ui.selectable_value(&mut d.ntsc, false, crate::i18n::tr(pal));
            ui.selectable_value(&mut d.ntsc, true, crate::i18n::tr(ntsc));
            if d.machine == Machine::C64 {
                ui.separator();
                ui.radio_value(&mut d.kernal, false, crate::i18n::tr("KERNAL out ($FFFE)"));
                ui.radio_value(&mut d.kernal, true, crate::i18n::tr("KERNAL in ($0314)"));
            }
            ui.separator();
            ui.label(crate::i18n::tr("Labels start"));
            ui.add(egui::TextEdit::singleline(&mut d.prefix).desired_width(70.0));
        });
        // Lines that no longer fit after a change of machine or of PAL and NTSC are brought back on.
        let usable: Vec<u16> = (0..d.frame_lines()).map(|r| d.line_of_row(r)).filter(|l| d.usable(*l)).collect();
        let clamp = |line: &mut u16| {
            if !usable.contains(line) {
                *line = usable.iter().copied().min_by_key(|l| (i32::from(*l) - i32::from(*line)).abs()).unwrap_or(*line);
            }
        };
        d.bars.iter_mut().for_each(|b| clamp(&mut b.top));
        d.splits.iter_mut().for_each(|s| clamp(&mut s.line));
        ui.horizontal_top(|ui| {
            egui::ScrollArea::vertical().id_salt("raster-picture").max_height(560.0).show(ui, |ui| self.picture(ui));
            ui.vertical(|ui| {
                ui.set_width(380.0);
                ui.horizontal(|ui| {
                    let d = &mut self.design;
                    if ui.button(crate::i18n::tr("Add a bar")).clicked() {
                        let top = self.chosen.and_then(|c| match c {
                            Chosen::Bar(i) => d.bars.get(i).map(|b| b.top + b.height + 8),
                            Chosen::Split(i) => d.splits.get(i).map(|s| s.line + 8),
                        });
                        let mut bar = Design::new(d.machine).bars.remove(0);
                        bar.top = top.filter(|t| d.usable(*t)).unwrap_or(bar.top);
                        d.bars.push(bar);
                        self.chosen = Some(Chosen::Bar(d.bars.len() - 1));
                    }
                    if ui.button(crate::i18n::tr("Add a split")).clicked() {
                        let mut split = Design::new(d.machine).splits.remove(0);
                        let below = d.splits.iter().map(|s| s.line + 16).max().filter(|l| d.usable(*l));
                        split.line = below.unwrap_or(split.line);
                        d.splits.push(split);
                        self.chosen = Some(Chosen::Split(d.splits.len() - 1));
                    }
                    if self.chosen.is_some() && ui.button(crate::i18n::tr("Remove")).clicked() {
                        match self.chosen.take() {
                            Some(Chosen::Bar(i)) if i < d.bars.len() => {
                                d.bars.remove(i);
                            }
                            Some(Chosen::Split(i)) if i < d.splits.len() => {
                                d.splits.remove(i);
                            }
                            _ => {}
                        }
                    }
                });
                ui.separator();
                self.settings(ui);
                ui.separator();
                // The colours at the top of the frame, for every colour register the design changes.
                let d = &mut self.design;
                let used: Vec<u16> = d.top().keys().filter_map(|r| if let Reg::Colour(n) = r { Some(*n) } else { None }).collect();
                if !used.is_empty() {
                    ui.label(RichText::new(crate::i18n::tr("At the top of the frame")).strong());
                    let names = d.machine.colours();
                    let snapshot = d.clone();
                    for n in used {
                        ui.horizontal(|ui| {
                            ui.label(names.iter().find(|c| c.0 == n).map_or(String::new(), |c| c.1.clone()));
                            let rgb = d.base.entry(n).or_insert([0, 0, 0]);
                            Self::colour(ui, &snapshot, rgb);
                        });
                    }
                }
                let registers: Vec<(u32, u32)> = d.top().into_iter().filter_map(|(r, v)| if let Reg::Register(a) = r { Some((a, v)) } else { None }).collect();
                for (address, mut value) in registers {
                    ui.horizontal(|ui| {
                        ui.label(d.machine.registers().iter().find(|r| r.1 == address).map_or(String::new(), |r| crate::i18n::tr(r.0).to_string()));
                        let most = if d.machine.is_amiga() { 0xFFFF } else { 0xFF };
                        if ui.add(egui::DragValue::new(&mut value).range(0..=most).hexadecimal(if most > 0xFF { 4 } else { 2 }, false, true).prefix("$")).changed() {
                            d.base_registers.insert(address, value);
                        }
                    });
                }
                ui.separator();
                match d.source() {
                    Ok(source) => {
                        ui.horizontal(|ui| {
                            if ui.button(crate::i18n::tr("Put the code at the cursor")).clicked() {
                                asked = Some(Asked::Insert(source.clone()));
                            }
                            if ui.button(crate::i18n::tr("Copy")).clicked() {
                                ui.ctx().copy_text(crate::i18n::comments(&source));
                            }
                            if d.machine.is_amiga() && ui.button(crate::i18n::tr("A program to try it")).on_hover_text(crate::i18n::tr("A new file: a whole Amiga program showing the list, to run with F9")).clicked() {
                                asked = Some(Asked::Program(d.amiga_program()));
                            }
                        });
                        let count = source.lines().count();
                        ui.label(RichText::new(crate::i18n::tf!("{count} lines of code")).small().color(ui.visuals().weak_text_color()));
                        egui::ScrollArea::both().id_salt("raster-source").max_height(260.0).show(ui, |ui| {
                            ui.add(egui::Label::new(RichText::new(&source).monospace().size(11.0)).extend());
                        });
                    }
                    Err(why) => {
                        ui.colored_label(ui.visuals().error_fg_color, why);
                    }
                }
            });
        });
        asked
    }
}

/// Whether a bar's register can be taken away: all but its first.
fn bar_targets_removable(k: usize) -> bool {
    k > 0
}

/// A line, as the machine's people write it: the Amiga's in hex, the others' in decimal.
fn line_words(design: &Design, line: u16) -> String {
    match design.machine {
        Machine::Amiga | Machine::AmigaAga => crate::i18n::tf!("line ${line:02X}"),
        Machine::C64 => crate::i18n::tf!("line {line} (${line:03X})"),
        Machine::St | Machine::Ste => crate::i18n::tf!("display line {line}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assemblers::{Cpu, assemble_own};
    use std::path::Path;

    fn bar(top: u16, height: u16, colours: &[[u8; 3]], targets: &[u16]) -> Bar {
        Bar { top, height, colours: colours.to_vec(), targets: targets.to_vec() }
    }

    fn design(machine: Machine, bars: Vec<Bar>, splits: Vec<Split>) -> Design {
        Design { bars, splits, ..Design::new(machine) }
    }

    #[test]
    fn colours_are_made_the_nearest_each_machine_has() {
        let amiga = Design::new(Machine::Amiga);
        assert_eq!(amiga.hardware([0xFF, 0x80, 0x00]), 0xF80);
        assert_eq!(amiga.shown(0xF80), [0xFF, 0x88, 0x00]);
        let aga = Design::new(Machine::AmigaAga);
        assert_eq!(aga.hardware([0x12, 0x34, 0x56]), 0x123456);
        let st = Design::new(Machine::St);
        assert_eq!(st.hardware([0xFF, 0x80, 0x00]), 0x740);
        assert_eq!(st.shown(0x700), [255, 0, 0]);
        // The STE's lowest bit is at the top: 15 is $F, 14 is $7, 1 is $8.
        let ste = Design::new(Machine::Ste);
        assert_eq!(ste.hardware([0xFF, 0xEE, 0x11]), 0xF78);
        assert_eq!(ste.shown(0xF78), [0xFF, 0xEE, 0x11]);
        let c64 = Design::new(Machine::C64);
        assert_eq!(c64.hardware([250, 250, 250]), 1, "white");
        assert_eq!(c64.hardware([0x30, 0x20, 0x80]), 6, "blue");
    }

    #[test]
    fn only_what_changes_is_written_and_bars_put_things_back() {
        let d = design(Machine::Amiga, vec![bar(0x40, 4, &[[0, 0, 0], [0xFF, 0xFF, 0xFF]], &[0])], vec![Split { line: 0x42, changes: vec![Change::Colour { index: 0, rgb: [0xFF, 0, 0] }] }]);
        let writes = d.writes();
        let colours: Vec<(u16, u32)> = writes.iter().map(|(l, w)| (*l, if let Write::Set(_, v) = w[0] { v } else { 0 })).collect();
        // Line $40 is black, as above it, so nothing's written; then the gradient, then red from the split.
        assert_eq!(colours, [(0x41, 0x555), (0x42, 0xAAA), (0x43, 0xFFF), (0x44, 0xF00)]);
    }

    /// The copper list run as the hardware would: each bar's line and the split show their colours, and a bar across
    /// line 255 has one WAIT for $FFDF before its first line past 255.
    #[test]
    fn the_copper_list_shows_the_design_and_crosses_line_255() {
        let d = design(Machine::Amiga, vec![bar(0xF8, 16, &[[0, 0, 0xFF], [0xFF, 0, 0]], &[0])], vec![Split { line: 0x120, changes: vec![Change::Colour { index: 0, rgb: [0, 0xFF, 0] }, Change::Colour { index: 1, rgb: [0xFF, 0xFF, 0] }] }]);
        let ops: Vec<Copper> = d.copper().into_iter().map(|(op, _)| op).collect();
        let wraps: Vec<usize> = ops.iter().enumerate().filter(|(_, o)| matches!(o, Copper::Wait { line: 0xFF, x: 0xDE, .. })).map(|(i, _)| i).collect();
        assert_eq!(wraps.len(), 1);
        assert!(matches!(ops[wraps[0] - 2], Copper::Wait { line: 0xFF, x: 0x06, .. }), "line 255 itself first");
        assert!(matches!(ops[wraps[0] + 1], Copper::Wait { line: 0x00, x: 0x06, .. }), "then line 256");
        let preview = crate::amiga_copper::simulate(&ops.iter().map(|o| Some(*o)).collect::<Vec<_>>());
        assert_eq!(preview.at(0xF8, 100), [0, 0, 0xFF]);
        assert_eq!(preview.at(0xF8 + 15, 100), [0xFF, 0, 0]);
        assert_eq!(preview.at(0x110, 100), [0, 0, 0], "back to black below the bar");
        assert_eq!(preview.at(0x120, 100), [0, 0xFF, 0]);
        assert_eq!(preview.palettes[0x120][0], [0xFF, 0xFF, 0], "COLOR01 from the split");
        // Every change lands at the start of its line, before the display window.
        assert_eq!(preview.at(0x100, 0x20), d.shown(d.bar_colour(&d.bars[0], 0x100)));
        // The source assembles to the same words.
        let source = d.source().unwrap();
        assert!(source.contains("        dc.w    $FFDF,$FFFE"), "{source}");
        let assembly = assemble_own(Cpu::M68k, Path::new("c.s"), &source).assembly;
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let words: Vec<u16> = ops.iter().flat_map(|o| {
            let (a, b) = o.encode();
            [a, b]
        }).collect();
        let bytes = assembly.image(assembly.lowest().unwrap(), assembly.highest().unwrap());
        assert_eq!(bytes.chunks(2).map(|w| u16::from_be_bytes([w[0], w[1]])).collect::<Vec<_>>(), words);
    }

    #[test]
    fn aga_colours_set_their_low_bits_and_ntsc_has_fewer_lines() {
        let mut d = design(Machine::AmigaAga, vec![], vec![Split { line: 0x50, changes: vec![Change::Colour { index: 0, rgb: [0x12, 0x34, 0x56] }] }]);
        let preview = crate::amiga_copper::simulate(&d.copper().into_iter().map(|(op, _)| Some(op)).collect::<Vec<_>>());
        assert_eq!(preview.at(0x50, 100), [0x12, 0x34, 0x56]);
        d.ntsc = true;
        assert_eq!(d.frame_lines(), 263);
        assert!(!d.usable(263) && d.usable(262));
    }

    #[test]
    fn bitplane_pointers_and_registers_split_the_screen() {
        let d = design(Machine::Amiga, vec![], vec![Split { line: 0xA0, changes: vec![Change::Register { address: 0x100, value: 0x9200 }, Change::Planes { count: 2, label: "lower".into(), bytes: 80 * 100 }] }]);
        let source = d.source().unwrap();
        assert!(source.contains("raster_planes1:\n        dc.w    $00E0,$0000"), "{source}");
        assert!(source.contains("raster_set_planes:"));
        let program = format!("{source}\nlower: ds.b 16000\n");
        let assembly = assemble_own(Cpu::M68k, Path::new("p.s"), &program).assembly;
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let ops: Vec<Copper> = d.copper().into_iter().map(|(op, _)| op).collect();
        assert_eq!(ops[0], Copper::Move { register: 0x100, value: 0x0200 }, "BPLCON0 as usual at the top");
        assert!(ops.contains(&Copper::Move { register: 0x100, value: 0x9200 }));
        assert_eq!(ops.iter().filter(|o| matches!(o, Copper::Move { register: 0xE0..=0xEE, .. })).count(), 4);
        // The program to try has the routine called, and assembles into an Amiga executable.
        let try_it = d.amiga_program().replace("nosprite:", "lower: ds.b 16000\nnosprite:");
        assert!(try_it.contains("bsr     raster_set_planes"));
        let made = crate::assemblers::assemble_own_to_run(Cpu::M68k, Path::new("t.s"), &try_it);
        assert!(made.assembly.ok(), "{:?}", made.assembly.problems);
        assert!(made.tape.is_some(), "an Amiga executable");
    }

    #[test]
    fn the_program_to_try_assembles() {
        for machine in [Machine::Amiga, Machine::AmigaAga] {
            let d = Design::new(machine);
            let program = d.amiga_program();
            assert!(!program.contains("$4007,$fffe"), "the starter's own list is gone");
            let made = crate::assemblers::assemble_own_to_run(Cpu::M68k, Path::new("t.s"), &program);
            assert!(made.assembly.ok(), "{:?}", made.assembly.problems);
            assert!(made.tape.is_some());
        }
    }

    /// The C64's code run on the 6502, with a VIC-II that only counts lines (63 cycles each, PAL) and interrupts
    /// at the line $D012 and $D011 ask for: every store lands in the first cycles of the line it's for, and the
    /// colours each line has are the design's.
    struct Vic2 {
        memory: Vec<u8>,
        cycles: u64,
        compare: u16,
        latched: bool,
        enabled: bool,
        /// Each store to a VIC-II register: when (in cycles from the start), the line, the cycle in the line, the
        /// register, the value.
        stores: Vec<(u64, u16, u64, u16, u8)>,
    }

    impl Vic2 {
        fn line(&self) -> u16 {
            (self.cycles / 63 % 312) as u16
        }
    }

    impl crate::m6502_cpu::Bus for Vic2 {
        fn read(&mut self, address: u16) -> u8 {
            self.cycles += 1;
            match address {
                0xD012 => self.line() as u8,
                0xD011 => (self.memory[0xD011] & 0x7F) | ((self.line() >> 1) & 0x80) as u8,
                0xD019 => u8::from(self.latched),
                _ => self.memory[usize::from(address)],
            }
        }
        fn write(&mut self, address: u16, value: u8) {
            self.cycles += 1;
            match address {
                0xD012 => self.compare = self.compare & 0x100 | u16::from(value),
                0xD011 => {
                    self.compare = self.compare & 0xFF | u16::from(value & 0x80) << 1;
                    self.memory[0xD011] = value;
                }
                0xD019 => self.latched &= value & 1 == 0,
                0xD01A => self.enabled = value & 1 != 0,
                _ => {
                    if (0xD015..=0xD024).contains(&address) {
                        self.stores.push((self.cycles - 1, self.line(), (self.cycles - 1) % 63, address, value));
                    }
                    self.memory[usize::from(address)] = value;
                }
            }
        }
    }

    fn run_c64(d: &Design, frames: u64) -> Vic2 {
        let source = d.source().unwrap();
        let program = format!("* = $1000\nstart\n jsr {p}_setup\nforever\n jmp forever\n{source}", p = d.prefix);
        let assembly = assemble_own(Cpu::M6502, Path::new("r.a"), &program).assembly;
        assert!(assembly.ok(), "{:?}\n{program}", assembly.problems);
        let mut vic = Vic2 { memory: assembly.image(0, 0x10000), cycles: 0, compare: 0, latched: false, enabled: false, stores: Vec::new() };
        let mut cpu = crate::m6502_cpu::Cpu { s: 0xFF, pc: 0x1000, ..Default::default() };
        let mut last_line = vic.line();
        while vic.cycles < frames * 312 * 63 {
            cpu.step(&mut vic);
            let line = vic.line();
            if line != last_line {
                last_line = line;
                if line == vic.compare {
                    vic.latched = true;
                }
            }
            // An interrupt: the 6502 pushes where it was and its flags, and goes through $FFFE, in 7 cycles.
            if vic.latched && vic.enabled && !cpu.i {
                let pc = cpu.pc.to_be_bytes();
                for byte in [pc[0], pc[1], cpu.status(false)] {
                    vic.memory[0x100 | usize::from(cpu.s)] = byte;
                    cpu.s = cpu.s.wrapping_sub(1);
                }
                cpu.i = true;
                cpu.pc = u16::from_le_bytes([vic.memory[0xFFFE], vic.memory[0xFFFF]]);
                vic.cycles += 7;
            }
        }
        vic
    }

    #[test]
    fn c64_raster_interrupts_change_colours_on_their_lines() {
        let d = Design {
            base: [(0, [0, 0, 0]), (1, [0x30, 0x28, 0x80])].into_iter().collect(),
            ..design(Machine::C64, vec![bar(0x60, 8, &[[0, 0, 0], [255, 255, 255]], &[0, 1])], vec![Split { line: 0x100, changes: vec![Change::Colour { index: 1, rgb: [0x68, 0x37, 0x2B] }, Change::Register { address: 0xD018, value: 0x17 }] }])
        };
        let vic = run_c64(&d, 4);
        // Every store but the top of the frame's (in the vertical blank) lands before the picture starts, cycle 17.
        let late: Vec<_> = vic.stores.iter().filter(|s| s.2 > 16 && s.1 != 300).collect();
        assert!(late.is_empty(), "every change early in its line: {late:?}");
        // What each register holds on each line of the third frame, by the middle of the line.
        let frame = 312 * 63 * 2;
        let held = |line: u64, register: u16| vic.stores.iter().rev().find(|s| s.3 == register && s.0 <= frame + line * 63 + 30).map(|s| u32::from(s.4));
        let rows = d.rows();
        for line in 16..300u16 {
            let wanted = &rows[usize::from(d.row_of(line))];
            for (n, register) in [(0, 0xD020), (1, 0xD021)] {
                assert_eq!(held(u64::from(line), register), wanted.get(&Reg::Colour(n)).copied(), "line {line}, ${register:04X}");
            }
        }
        // The bar from black through the greys to white, then back to the border's own colour.
        let border: Vec<u32> = (0x60..0x69).map(|l| held(l, 0xD020).unwrap()).collect();
        assert_eq!((border[0], border[7], border[8]), (0, 1, 0), "{border:?}");
        assert!(!border[1..7].contains(&0), "greys between: {border:?}");
        assert_eq!(held(0x100, 0xD021), Some(2), "red from line 256");
        assert_eq!(vic.memory[0xD018], 0x15, "back to the usual at the top of the frame");
        assert!(vic.stores.iter().any(|s| s.1 == 0x100 && s.3 == 0xD018 && s.4 == 0x17));
    }

    #[test]
    fn c64_code_assembles_with_the_kernal_in_and_on_ntsc() {
        for (kernal, ntsc) in [(true, false), (false, true), (true, true)] {
            let d = Design { kernal, ntsc, ..Design::new(Machine::C64) };
            let source = d.source().unwrap();
            let assembly = assemble_own(Cpu::M6502, Path::new("r.a"), &format!("* = $c000\n{source}")).assembly;
            assert!(assembly.ok(), "{:?}\n{source}", assembly.problems);
        }
        // NTSC's frame starts at its vertical blank (line 13): lines 0 to 12 are the bottom of the picture.
        let d = Design { ntsc: true, ..Design::new(Machine::C64) };
        assert_eq!(d.row_of(13), 0);
        assert!(d.row_of(5) > d.row_of(200));
        assert!(!d.usable(20) && d.usable(5));
    }

    #[test]
    fn st_timer_b_counts_the_lines_between_changes() {
        let d = design(Machine::St, vec![bar(40, 3, &[[0xFF, 0, 0], [0, 0, 0xFF]], &[0])], vec![Split { line: 100, changes: vec![Change::Colour { index: 0, rgb: [0, 0xFF, 0] }, Change::Register { address: 0xFF8260, value: 1 }] }]);
        let source = d.source().unwrap();
        let assembly = assemble_own(Cpu::M68k, Path::new("st.s"), &source).assembly;
        assert!(assembly.ok(), "{:?}\n{source}", assembly.problems);
        // Changes on lines 40, 41, 42 (the bar), 43 (back to black) and 100: each handler counts on to the next.
        let lines: Vec<u16> = d.writes().iter().map(|(l, _)| *l).collect();
        assert_eq!(lines, [40, 41, 42, 43, 100]);
        let counts: Vec<&str> = source.lines().filter(|l| l.contains(",$fffffa21.w")).map(|l| l.split_whitespace().nth(1).unwrap()).collect();
        assert_eq!(counts, ["#40,$fffffa21.w", "#40,$fffffa21.w", "#1,$fffffa21.w", "#1,$fffffa21.w", "#1,$fffffa21.w", "#57,$fffffa21.w"], "setup, the VBL, then each handler");
        assert!(source.contains("move.w  #$700,$ffff8240.w"));
        assert!(source.contains("move.b  #$01,$ffff8260.w"));
        assert_eq!(source.lines().filter(|l| l.trim() == "rte").count(), 5);
        // The last stops the timer until the next frame.
        let last = source.rsplit("; Display line 100.").next().unwrap();
        assert!(last.contains("clr.b   $fffffa1b.w") && !last.contains("$fffffa21"));
    }

    #[test]
    fn the_picture_follows_the_design() {
        let d = design(Machine::St, vec![], vec![Split { line: 100, changes: vec![Change::Colour { index: 0, rgb: [0xFF, 0xFF, 0xFF] }] }]);
        let rows = d.rows();
        assert_eq!(rows[usize::from(d.row_of(99))][&Reg::Colour(0)], 0);
        assert_eq!(rows[usize::from(d.row_of(100))][&Reg::Colour(0)], 0x777);
        assert!(d.row_hidden(0) && !d.row_hidden(d.row_of(0)));
        assert_eq!(d.line_of_row(d.row_of(57)), 57);
    }

    /// With LIMONITER_RASTER_OUT set to a folder: the program to try a design that crosses line 255, written there
    /// as an Amiga executable (`raster`) to run in FS-UAE and look at.
    #[test]
    fn a_program_to_run_in_an_emulator() {
        let Some(folder) = std::env::var_os("LIMONITER_RASTER_OUT") else {
            return;
        };
        let d = Design {
            base: [(0, [0x10, 0x10, 0x30])].into_iter().collect(),
            ..design(
                Machine::Amiga,
                vec![bar(0x40, 32, &[[0, 0, 0x40], [0xFF, 0xFF, 0xFF], [0, 0, 0x40]], &[0]), bar(0xF0, 32, &[[0xFF, 0, 0], [0xFF, 0xFF, 0], [0, 0xFF, 0]], &[0])],
                vec![Split { line: 0x90, changes: vec![Change::Colour { index: 0, rgb: [0x60, 0x00, 0x60] }] }, Split { line: 0x120, changes: vec![Change::Colour { index: 0, rgb: [0x00, 0x80, 0x80] }] }],
            )
        };
        let program = d.amiga_program();
        let made = crate::assemblers::assemble_own_to_run(Cpu::M68k, Path::new("raster.s"), &program);
        assert!(made.assembly.ok(), "{:?}", made.assembly.problems);
        let folder = std::path::PathBuf::from(folder);
        std::fs::write(folder.join("raster.s"), &program).unwrap();
        std::fs::write(folder.join("raster"), made.tape.unwrap().1).unwrap();
        // And a C64 program, from a BASIC line: bars on the border and background, and a split.
        let d = Design {
            base: [(0, [0x6C, 0x5E, 0xB5]), (1, [0x35, 0x28, 0x79])].into_iter().collect(),
            ..design(
                Machine::C64,
                vec![bar(0x40, 24, &[[0, 0, 0], [255, 255, 255], [0, 0, 0]], &[0, 1]), bar(0xF0, 24, &[[0x88, 0x39, 0], [0xFF, 0xFF, 0x80], [0x88, 0x39, 0]], &[0])],
                vec![Split { line: 0xA0, changes: vec![Change::Colour { index: 1, rgb: [0x58, 0x8D, 0x43] }] }],
            )
        };
        let program = format!("* = $0801\n !word +, 10\n !byte $9e\n !text \"2061\"\n !byte 0\n+ !word 0\nstart\n jsr raster_setup\nmain\n jmp main\n{}", d.source().unwrap());
        let assembly = assemble_own(Cpu::M6502, Path::new("raster.a"), &program).assembly;
        assert!(assembly.ok(), "{:?}", assembly.problems);
        std::fs::write(folder.join("raster.a"), &program).unwrap();
        std::fs::write(folder.join("raster.prg"), crate::asm::output::commodore_program(&assembly).unwrap()).unwrap();
        // And an ST program: the ST demo starter, its VBL calling raster_vbl and its own bars taken out.
        let d = design(
            Machine::St,
            vec![bar(30, 40, &[[0, 0, 0x80], [0xFF, 0xFF, 0xFF], [0, 0, 0x80]], &[0])],
            vec![Split { line: 120, changes: vec![Change::Colour { index: 0, rgb: [0x60, 0, 0x60] }] }, Split { line: 160, changes: vec![Change::Colour { index: 0, rgb: [0, 0x80, 0x80] }] }],
        );
        let starter = crate::st_starters::ST_DEMO;
        let vbl = "        clr.b   TBCR.w                  ; stop Timer B\n        move.b  #BAR_LINES,TBDR.w       ; an interrupt every BAR_LINES lines\n        move.b  #8,TBCR.w               ; event count mode: it counts display lines\n";
        let colour = "        move.w  (a0)+,COLOR0.w          ; the colour above the first change\n";
        assert!(starter.contains(vbl) && starter.contains(colour));
        let program = starter.replace(vbl, "        bsr     raster_vbl\n").replace(colour, "").replace("draw_screen:\n", &format!("{}\ndraw_screen:\n", d.source().unwrap()));
        let no_files = |p: &Path| Err(crate::i18n::tf!("{} isn't there", p.display()));
        let here = crate::asm::assemble(&crate::asm::m68k::M68k, Path::new("st.s"), &program, &no_files);
        assert!(here.ok(), "{:?}", here.problems);
        let moved = crate::asm::assemble_at(&crate::asm::m68k::M68k, Path::new("st.s"), &program, &no_files, 0x10000);
        std::fs::write(folder.join("raster_st.s"), &program).unwrap();
        std::fs::write(folder.join("RASTER.PRG"), crate::asm::output::atari_program(&here, &moved, 0x10000).unwrap()).unwrap();
    }

    #[test]
    fn the_window_draws() {
        let ctx = egui::Context::default();
        for machine in Machine::ALL {
            let mut designer = Designer::for_machine(machine);
            for chosen in [Some(Chosen::Bar(0)), Some(Chosen::Split(0)), None] {
                designer.chosen = chosen;
                let _ = ctx.run_ui(Default::default(), |ui| {
                    egui::Window::new("t").show(ui.ctx(), |ui| designer.draw(ui));
                });
            }
        }
    }
}
