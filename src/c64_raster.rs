//! The raster line calculator, for timing C64 code to the VIC-II: for a PAL or NTSC machine, a raster line, the
//! vertical scroll and the sprites shown on it, where the line is (vertical blank, border or the display window),
//! whether it's a bad line, which cycles the VIC-II takes for character pointers and sprite data, and how many the
//! processor has left.
//!
//! The VIC-II takes the bus in a bad line for 40 cycles (15 to 54) to read the row's character pointers, and for
//! 2 cycles for each sprite shown, to read its 3 bytes. It warns the processor 3 cycles before (BA goes low), and
//! the processor stops at its next read: in those 3 cycles it can only go on writing, which at best it does (a
//! 6510 writes at most 3 cycles running). So each line has a best and a worst case: the worst counts all 3 lost.

use eframe::egui::{self, Color32, RichText, Sense, Ui, vec2};

/// Which VIC-II.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vic {
    /// The PAL 6569: 63 cycles a line, 312 lines.
    Pal,
    /// The NTSC 6567R8: 65 cycles a line, 263 lines.
    Ntsc,
    /// The first NTSC 6567R56A: 64 cycles a line, 262 lines.
    OldNtsc,
}

impl Vic {
    pub const ALL: [Vic; 3] = [Vic::Pal, Vic::Ntsc, Vic::OldNtsc];

    pub fn name(self) -> &'static str {
        match self {
            Vic::Pal => "PAL (6569)",
            Vic::Ntsc => "NTSC (6567R8)",
            Vic::OldNtsc => "Old NTSC (6567R56A)",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Vic::Pal => "PAL",
            Vic::Ntsc => "NTSC",
            Vic::OldNtsc => "old NTSC",
        }
    }

    pub fn cycles(self) -> u32 {
        match self {
            Vic::Pal => 63,
            Vic::Ntsc => 65,
            Vic::OldNtsc => 64,
        }
    }

    pub fn lines(self) -> u32 {
        match self {
            Vic::Pal => 312,
            Vic::Ntsc => 263,
            Vic::OldNtsc => 262,
        }
    }

    /// The processor's clock, in Hz.
    pub fn clock(self) -> f64 {
        match self {
            Vic::Pal => 985_248.0,
            Vic::Ntsc | Vic::OldNtsc => 1_022_727.0,
        }
    }

    /// The cycle (from 1) of sprite 0's first data read; the others follow 2 cycles apart, running on into the
    /// next line, where sprite 3's is in cycle 1.
    fn sprite_zero(self) -> u32 {
        match self {
            Vic::Pal => 58,
            Vic::Ntsc => 60,
            Vic::OldNtsc => 59,
        }
    }

    /// The lines of vertical blank, where nothing is shown: from the first to the last, through line 0 on PAL.
    pub fn blank(self) -> (u32, u32) {
        match self {
            Vic::Pal => (300, 15),
            Vic::Ntsc | Vic::OldNtsc => (13, 40),
        }
    }

    /// Whether a line is in the vertical blank.
    pub fn is_blank(self, line: u32) -> bool {
        let (first, last) = self.blank();
        if first > last { line >= first || line <= last } else { (first..=last).contains(&line) }
    }

    /// How many lines are shown.
    pub fn visible(self) -> u32 {
        (0..self.lines()).filter(|l| !self.is_blank(*l)).count() as u32
    }
}

/// What the VIC-II's doing in a cycle, as far as the processor's concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    /// The processor has it.
    Cpu,
    /// BA is low: the processor can still write, but stops at its first read.
    Warned,
    /// The VIC-II reads character pointers (on a bad line).
    Chars,
    /// The VIC-II reads a sprite's data.
    Sprite(u8),
}

/// Where a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Blank,
    TopBorder,
    Display,
    BottomBorder,
}

impl Area {
    pub fn name(self) -> &'static str {
        match self {
            Area::Blank => "vertical blank (not shown)",
            Area::TopBorder => "the top border",
            Area::Display => "the display window",
            Area::BottomBorder => "the bottom border",
        }
    }
}

/// What's asked about a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setup {
    pub vic: Vic,
    pub line: u32,
    /// $D011's low 3 bits.
    pub yscroll: u8,
    /// $D011 bit 4, the screen on (it has to be on at line $30 for there to be bad lines).
    pub screen_on: bool,
    /// 25 rows (bit 3 of $D011 set) or 24.
    pub rows25: bool,
    /// The sprites shown on the line, a bit each.
    pub sprites: u8,
}

impl Default for Setup {
    fn default() -> Self {
        Setup { vic: Vic::Pal, line: 0x33, yscroll: 3, screen_on: true, rows25: true, sprites: 0 }
    }
}

/// The first and last lines of the display window.
pub fn window(rows25: bool) -> (u32, u32) {
    if rows25 { (0x33, 0xFA) } else { (0x37, 0xF6) }
}

/// Whether a line is a bad line: between $30 and $F7, with its low 3 bits the vertical scroll, and the screen on.
pub fn is_bad_line(line: u32, yscroll: u8, screen_on: bool) -> bool {
    screen_on && (0x30..=0xF7).contains(&line) && (line & 7) as u8 == yscroll & 7
}

/// All the bad lines of a frame.
pub fn bad_lines(yscroll: u8, screen_on: bool) -> Vec<u32> {
    (0x30..=0xF7).filter(|l| is_bad_line(*l, yscroll, screen_on)).collect()
}

/// What a line comes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub area: Area,
    pub bad: bool,
    /// Each cycle's use, from cycle 1.
    pub cycles: Vec<Use>,
    /// Cycles the processor has: at worst (it reads as soon as BA is low) and at best (it writes for up to 3).
    pub worst: u32,
    pub best: u32,
    /// Cycles the VIC-II reads in: for the bad line, and for sprites.
    pub chars: u32,
    pub sprite_reads: u32,
}

impl Setup {
    pub fn area(&self) -> Area {
        let (top, bottom) = window(self.rows25);
        let (blank_from, blank_to) = self.vic.blank();
        if self.vic.is_blank(self.line) {
            Area::Blank
        } else if blank_from <= blank_to && self.line < blank_from {
            // On NTSC the bottom border runs on past the last line into the next frame's first.
            Area::BottomBorder
        } else if self.line < top {
            Area::TopBorder
        } else if self.line <= bottom {
            Area::Display
        } else {
            Area::BottomBorder
        }
    }

    /// The line, cycle by cycle. Sprites are counted as in a line through the middle of them: sprites 0 to 2 are
    /// read at the end of the line before the one they show on, and 3 to 7 at its start, so a line among a
    /// sprite's rows has both.
    pub fn report(&self) -> Report {
        let n = self.vic.cycles() as usize;
        let mut cycles = vec![Use::Cpu; n];
        let bad = is_bad_line(self.line, self.yscroll, self.screen_on);
        if bad {
            for c in &mut cycles[14..54] {
                *c = Use::Chars;
            }
        }
        for sprite in 0..8u8 {
            if self.sprites & (1 << sprite) == 0 {
                continue;
            }
            let first = (self.vic.sprite_zero() as usize - 1 + 2 * sprite as usize) % n;
            cycles[first] = Use::Sprite(sprite);
            cycles[(first + 1) % n] = Use::Sprite(sprite);
        }
        // BA goes low 3 cycles before each read.
        let reads: Vec<usize> = (0..n).filter(|c| cycles[*c] != Use::Cpu).collect();
        for c in reads {
            for back in 1..=3 {
                let before = (c + n - back) % n;
                if cycles[before] == Use::Cpu {
                    cycles[before] = Use::Warned;
                }
            }
        }
        let worst = cycles.iter().filter(|u| **u == Use::Cpu).count() as u32;
        // At best the processor writes through the warned cycles that start each stretch the VIC-II takes; once
        // it's stopped it stays stopped until BA is high again.
        let mut best = worst;
        if worst < n as u32 {
            let start = (0..n).find(|c| cycles[*c] == Use::Cpu).unwrap_or(0);
            let mut run_start = true;
            for i in 0..n {
                let c = cycles[(start + i) % n];
                match c {
                    Use::Cpu => run_start = true,
                    Use::Warned if run_start => best += 1,
                    _ => run_start = false,
                }
            }
        } else {
            best = worst;
        }
        let chars = cycles.iter().filter(|u| **u == Use::Chars).count() as u32;
        let sprite_reads = cycles.iter().filter(|u| matches!(u, Use::Sprite(_))).count() as u32;
        Report { area: self.area(), bad, cycles, worst, best, chars, sprite_reads }
    }

    /// The processor's cycles in a whole frame with these settings, the sprites on no line: at worst and best.
    pub fn frame(&self) -> (u32, u32) {
        let mut worst = 0;
        let mut best = 0;
        for line in 0..self.vic.lines() {
            let r = Setup { line, sprites: 0, ..*self }.report();
            worst += r.worst;
            best += r.best;
        }
        (worst, best)
    }

    /// A comment saying what the line comes to, to put in the source.
    pub fn note(&self) -> String {
        let r = self.report();
        let sprites: Vec<String> = (0..8).filter(|s| self.sprites & (1 << s) != 0).map(|s| s.to_string()).collect();
        let with = if sprites.is_empty() { "no sprites".to_string() } else { format!("sprite{} {}", if sprites.len() == 1 { "" } else { "s" }, sprites.join(", ")) };
        crate::i18n::tf!("; Line ${:03X} ({}) on {}, {}{}, {with}: the processor has {} cycles ({} if it's reading when the VIC-II takes the bus)\n",
            self.line,
            self.line,
            self.vic.short(),
            self.area().name(),
            if r.bad { ", a bad line" } else { "" },
            r.best,
            r.worst
        )
    }
}

/// The calculator's window.
#[derive(Default)]
pub struct Calculator {
    pub setup: Setup,
}

pub enum Asked {
    Insert(String),
}

fn colour(u: Use) -> Color32 {
    match u {
        Use::Cpu => Color32::from_rgb(0x4c, 0xa0, 0x5a),
        Use::Warned => Color32::from_rgb(0xd8, 0xb0, 0x40),
        Use::Chars => Color32::from_rgb(0xc0, 0x50, 0x50),
        Use::Sprite(_) => Color32::from_rgb(0x50, 0x78, 0xd0),
    }
}

impl Calculator {
    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        let s = &mut self.setup;
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Machine"));
            for vic in Vic::ALL {
                ui.selectable_value(&mut s.vic, vic, crate::i18n::tr(vic.name()));
            }
        });
        let last = s.vic.lines() - 1;
        s.line = s.line.min(last);
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Raster line"));
            ui.add(egui::DragValue::new(&mut s.line).range(0..=last));
            ui.label(crate::i18n::tf!("(${:03X})", s.line));
            ui.separator();
            ui.label(crate::i18n::tr("Vertical scroll"));
            ui.add(egui::DragValue::new(&mut s.yscroll).range(0..=7));
            ui.checkbox(&mut s.screen_on, crate::i18n::tr("Screen on")).on_hover_text(crate::i18n::tr("$D011 bit 4: with the screen off at line $30, there are no bad lines all frame"));
            ui.checkbox(&mut s.rows25, crate::i18n::tr("25 rows"));
        });
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Sprites on the line"));
            for sprite in 0..8u8 {
                let mut on = s.sprites & (1 << sprite) != 0;
                if ui.checkbox(&mut on, sprite.to_string()).changed() {
                    s.sprites ^= 1 << sprite;
                }
            }
        });
        let r = s.report();
        ui.separator();
        ui.label(crate::i18n::tf!(
            "{} cycles a line, {} lines a frame: {} cycles a frame, {:.2} frames a second.",
            s.vic.cycles(),
            s.vic.lines(),
            s.vic.cycles() * s.vic.lines(),
            s.vic.clock() / (s.vic.cycles() * s.vic.lines()) as f64
        ));
        let (top, bottom) = window(s.rows25);
        let (blank_from, blank_to) = s.vic.blank();
        ui.label(crate::i18n::tf!(
            "Display window: lines ${top:02X} to ${bottom:02X} ({top} to {bottom}). Vertical blank: lines {blank_from} to {blank_to}, so {} lines are shown.",
            s.vic.visible()
        ));
        let bad = bad_lines(s.yscroll, s.screen_on);
        if let (Some(first), Some(end)) = (bad.first(), bad.last()) {
            ui.label(crate::i18n::tf!("Bad lines: every 8th from ${first:02X} to ${end:02X}, {} of them.", bad.len()));
        } else {
            ui.label(crate::i18n::tr("No bad lines: the screen is off."));
        }
        ui.add_space(4.0);
        ui.label(RichText::new(crate::i18n::tf!("Line ${:03X} is in {}{}.", s.line, r.area.name(), if r.bad { ", and a bad line" } else { "" })).strong());
        ui.label(crate::i18n::tf!(
            "The processor has {} cycles of {}, or {} if it's reading when the VIC-II warns it. The VIC-II reads for {} ({} for the row's characters, {} for sprites).",
            r.best,
            s.vic.cycles(),
            r.worst,
            r.chars + r.sprite_reads,
            r.chars,
            r.sprite_reads
        ));
        // The line, a cell a cycle.
        let width = (ui.available_width().max(400.0) / r.cycles.len() as f32).clamp(5.0, 11.0);
        let (rect, response) = ui.allocate_exact_size(vec2(width * r.cycles.len() as f32, 26.0), Sense::hover());
        let painter = ui.painter_at(rect);
        for (i, u) in r.cycles.iter().enumerate() {
            let cell = egui::Rect::from_min_size(rect.min + vec2(i as f32 * width, 0.0), vec2(width - 1.0, 16.0));
            painter.rect_filled(cell, 1.0, colour(*u));
            if (i + 1) % 10 == 0 || i == 0 {
                painter.text(cell.center_bottom() + vec2(0.0, 1.0), egui::Align2::CENTER_TOP, (i + 1).to_string(), egui::FontId::proportional(9.0), ui.visuals().weak_text_color());
            }
        }
        if let Some(at) = response.hover_pos() {
            let i = (((at.x - rect.min.x) / width) as usize).min(r.cycles.len() - 1);
            let what = match r.cycles[i] {
                Use::Cpu => "the processor's".to_string(),
                Use::Warned => crate::i18n::tr("BA low: the processor can only write").to_string(),
                Use::Chars => crate::i18n::tr("the VIC-II reads a character pointer").to_string(),
                Use::Sprite(n) => crate::i18n::tf!("the VIC-II reads sprite {n}'s data"),
            };
            response.on_hover_text(crate::i18n::tf!("Cycle {}: {what}", i + 1));
        }
        ui.horizontal(|ui| {
            for (u, words) in [(Use::Cpu, "processor"), (Use::Warned, "BA low (writes only)"), (Use::Chars, "character pointers"), (Use::Sprite(0), "sprite data")] {
                let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                ui.painter().rect_filled(rect, 1.0, colour(u));
                ui.label(RichText::new(words).small());
            }
        });
        let (worst, best) = s.frame();
        ui.label(RichText::new(crate::i18n::tf!("A whole frame with no sprites gives the processor {best} cycles ({worst} at worst).")).small());
        ui.label(RichText::new(crate::i18n::tr("Sprites 0 to 2 are read at the end of the line before the one they show on, 3 to 7 at the start of it; the count is for a line among a sprite's rows.")).small());
        ui.add_space(4.0);
        if ui.button(crate::i18n::tr("Put a note at the cursor")).on_hover_text(crate::i18n::tr("A comment saying what this line leaves the processor")).clicked() {
            asked = Some(Asked::Insert(s.note()));
        }
        asked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(vic: Vic, line: u32, sprites: u8) -> Report {
        Setup { vic, line, sprites, ..Default::default() }.report()
    }

    #[test]
    fn a_bad_line_leaves_23_cycles_on_pal() {
        let r = line(Vic::Pal, 0x33, 0);
        assert!(r.bad);
        assert_eq!((r.best, r.worst, r.chars), (23, 20, 40));
        // Not a bad line: all 63.
        let r = line(Vic::Pal, 0x34, 0);
        assert_eq!((r.bad, r.best, r.worst), (false, 63, 63));
        // NTSC's lines are longer, by 2 cycles, and the old NTSC's by 1.
        assert_eq!(line(Vic::Ntsc, 0x33, 0).best, 25);
        assert_eq!(line(Vic::OldNtsc, 0x33, 0).best, 24);
    }

    #[test]
    fn each_sprite_takes_2_cycles_and_3_more_to_warn() {
        let r = line(Vic::Pal, 0x80, 0b0000_0001);
        assert_eq!((r.sprite_reads, r.best, r.worst), (2, 61, 58));
        // Sprites next to each other share the warning: 2 each and 3 once.
        let r = line(Vic::Pal, 0x80, 0b0000_0011);
        assert_eq!((r.best, r.worst), (63 - 4, 63 - 7));
        let r = line(Vic::Pal, 0x80, 0xFF);
        assert_eq!((r.sprite_reads, r.best, r.worst), (16, 63 - 16, 63 - 19));
        // With one between them off, the processor can't come back in the 2 cycles: they count as lost.
        let r = line(Vic::Pal, 0x80, 0b0000_0101);
        assert_eq!(r.worst, 63 - 9);
        // Two off between them leave room for the processor to run a cycle.
        let r = line(Vic::Pal, 0x80, 0b0000_1001);
        assert_eq!(r.worst, 63 - 10);
        assert_eq!(r.best, 63 - 4);
        // Sprite 3 is read at the start of the line, in cycles 1 and 2.
        let r = line(Vic::Pal, 0x80, 0b0000_1000);
        assert_eq!(r.cycles[0], Use::Sprite(3));
        assert_eq!(r.cycles[62], Use::Warned);
        // Sprite 0 in cycles 58 and 59 on PAL, 60 and 61 on NTSC.
        assert_eq!(line(Vic::Pal, 0x80, 1).cycles[57], Use::Sprite(0));
        assert_eq!(line(Vic::Ntsc, 0x80, 1).cycles[59], Use::Sprite(0));
    }

    #[test]
    fn a_bad_line_with_every_sprite_leaves_almost_nothing() {
        let r = line(Vic::Pal, 0x33, 0xFF);
        // Cycle 11 is the processor's, between sprite 7's read and BA going low for the bad line in cycle 12.
        assert_eq!(r.cycles[10], Use::Cpu);
        assert_eq!(r.worst, 1);
        assert_eq!(r.best, 4);
    }

    #[test]
    fn bad_lines_and_areas() {
        let bad = bad_lines(3, true);
        assert_eq!((bad.len(), bad[0], *bad.last().unwrap()), (25, 0x33, 0xF3));
        assert_eq!(bad_lines(0, true)[0], 0x30);
        assert_eq!(bad_lines(0, true).len(), 25);
        assert!(bad_lines(3, false).is_empty());
        let area = |line: u32, vic: Vic| Setup { vic, line, ..Default::default() }.area();
        assert_eq!(area(0x32, Vic::Pal), Area::TopBorder);
        assert_eq!(area(0x33, Vic::Pal), Area::Display);
        assert_eq!(area(0xFA, Vic::Pal), Area::Display);
        assert_eq!(area(0xFB, Vic::Pal), Area::BottomBorder);
        assert_eq!(area(300, Vic::Pal), Area::Blank);
        assert_eq!(area(10, Vic::Pal), Area::Blank);
        assert_eq!(area(20, Vic::Ntsc), Area::Blank);
        assert_eq!(area(5, Vic::Ntsc), Area::BottomBorder);
        assert_eq!((Vic::Pal.visible(), Vic::Ntsc.visible(), Vic::OldNtsc.visible()), (284, 235, 234));
        assert_eq!(window(false), (0x37, 0xF6));
    }

    #[test]
    fn a_frame_and_a_note() {
        let s = Setup::default();
        // 312 lines of 63, less 40 for each of 25 bad lines (43 at worst).
        assert_eq!(s.frame(), (312 * 63 - 25 * 43, 312 * 63 - 25 * 40));
        assert_eq!(s.note(), "; Line $033 (51) on PAL, the display window, a bad line, no sprites: the processor has 23 cycles (20 if it's reading when the VIC-II takes the bus)\n");
    }
}
