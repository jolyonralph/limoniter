//! The STE's (and Mega ST's and Falcon's) blitter, worked out: the halftone operation (HOP) and the 16 logic
//! operations (OP), the end masks for a run of pixels, and for a copy from one x to another, the skew and whether
//! it needs FXSR (an extra source read at the start of each line) or NFSR (no source read for the last word);
//! then the increments, counts and addresses, and the register writes that set them up.
//!
//! The rules are checked against a model of the blitter word by word, written after Hatari's emulation of it (in
//! the tests' build only).

use crate::st_hw::{HOPS, OPS};
use eframe::egui::{self, RichText, Ui};

/// What an operation makes of a source bit and a destination bit: Atari's numbering has bit 3 of OP for
/// S=0 D=0, bit 2 for S=0 D=1, bit 1 for S=1 D=0 and bit 0 for S=1 D=1, so each bit of the result is one bit of
/// OP chosen by the two inputs.
pub fn op(op: u8, s: u16, d: u16) -> u16 {
    let bit = |n: u8| if op & (1 << n) != 0 { 0xFFFF } else { 0 };
    (bit(0) & s & d) | (bit(1) & s & !d) | (bit(2) & !s & d) | (bit(3) & !s & !d)
}

/// Whether an operation reads the source, and the destination.
pub fn op_reads(op: u8) -> (bool, bool) {
    let op = op & 15;
    // It reads S if changing S can change the result, and D likewise.
    let s = (op & 3) != ((op >> 2) & 3);
    let d = (op & 5) != ((op >> 1) & 5);
    (s, d)
}

#[cfg(test)]
/// What the halftone operation makes the source: all ones, the halftone line, the source, or both ANDed.
pub fn hop(hop: u8, source: u16, halftone: u16) -> u16 {
    match hop & 3 {
        0 => 0xFFFF,
        1 => halftone,
        2 => source,
        _ => source & halftone,
    }
}

/// The blitter's registers, for the model below.
#[cfg(test)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Regs {
    pub halftone: [u16; 16],
    pub src_x_inc: i16,
    pub src_y_inc: i16,
    pub src_addr: u32,
    pub endmask: [u16; 3],
    pub dst_x_inc: i16,
    pub dst_y_inc: i16,
    pub dst_addr: u32,
    pub x_count: u16,
    pub y_count: u16,
    pub hop: u8,
    pub op: u8,
    /// $FF8A3C: smudge (bit 5) and the halftone line (bits 0-3).
    pub control: u8,
    /// $FF8A3D: FXSR (bit 7), NFSR (bit 6) and the skew (bits 0-3).
    pub skew: u8,
}

#[cfg(test)]
/// Runs a blit on memory (big-endian words, from address 0), as the blitter does it word by word: this follows
/// Hatari's emulation, without the bus timing, including what NFSR does on the last word of a line.
pub fn run(regs: &Regs, memory: &mut [u8]) {
    let read = |memory: &[u8], at: u32| -> u16 {
        let at = (at as usize) % memory.len();
        u16::from_be_bytes([memory[at], memory[(at + 1) % memory.len()]])
    };
    let mut r = regs.clone();
    let (fxsr, nfsr, skew) = (regs.skew & 0x80 != 0, regs.skew & 0x40 != 0, regs.skew & 15);
    let smudge = regs.control & 0x20 != 0;
    let mut halftone_line = regs.control & 15;
    let reset = if regs.x_count == 0 { 65536u32 } else { regs.x_count as u32 };
    let mut x = reset;
    let mut y = if regs.y_count == 0 { 65536u32 } else { regs.y_count as u32 };
    let mut buffer: u32 = 0;
    let mut bus: u16 = 0;
    let (mut line_nfsr, mut line_fxsr, mut have_fxsr) = (false, false, false);
    let backwards = r.src_x_inc < 0;
    let shift = |buffer: &mut u32| if backwards { *buffer >>= 16 } else { *buffer <<= 16 };
    let fetch = |buffer: &mut u32, word: u16| if backwards { *buffer |= (word as u32) << 16 } else { *buffer |= word as u32 };
    let (lop_src, lop_dst) = op_reads(r.op);
    while y > 0 {
        let first = x == reset;
        let mask = if first || reset == 1 {
            r.endmask[0]
        } else if x == 1 {
            r.endmask[2]
        } else {
            r.endmask[1]
        };
        if first {
            line_nfsr = false;
            line_fxsr = fxsr;
        }
        let need_src = lop_src && (r.hop & 2 != 0 || (r.hop == 1 && smudge));
        let need_dst = lop_dst || mask != 0xFFFF;
        let mut fetched = false;
        if line_fxsr && !have_fxsr && need_src {
            shift(&mut buffer);
            bus = read(memory, r.src_addr);
            fetch(&mut buffer, bus);
            r.src_addr = r.src_addr.wrapping_add(r.src_x_inc as i32 as u32);
            have_fxsr = true;
        }
        if need_src && !line_nfsr {
            shift(&mut buffer);
            bus = read(memory, r.src_addr);
            fetch(&mut buffer, bus);
            fetched = true;
        }
        let dst_word = if need_dst {
            bus = read(memory, r.dst_addr);
            bus
        } else {
            0
        };
        if nfsr && x == 1 {
            shift(&mut buffer);
            fetch(&mut buffer, bus);
        }
        let source = (buffer >> skew) as u16;
        let halftone = r.halftone[if smudge { (source & 15) as usize } else { halftone_line as usize }];
        let result = op(r.op, hop(r.hop, source, halftone), dst_word);
        let out = if mask != 0xFFFF { (result & mask) | (dst_word & !mask) } else { result };
        let at = (r.dst_addr as usize) % memory.len();
        memory[at] = (out >> 8) as u8;
        memory[(at + 1) % memory.len()] = out as u8;
        bus = out;
        if nfsr && x == 1 {
            shift(&mut buffer);
            fetch(&mut buffer, bus);
        }
        if x == 2 && nfsr {
            line_nfsr = true;
        }
        if fetched {
            let step = if x == 1 || line_nfsr { r.src_y_inc } else { r.src_x_inc };
            r.src_addr = r.src_addr.wrapping_add(step as i32 as u32);
        }
        if x == 1 {
            have_fxsr = false;
            y -= 1;
            x = reset;
            r.dst_addr = r.dst_addr.wrapping_add(r.dst_y_inc as i32 as u32);
            halftone_line = if r.dst_y_inc >= 0 { (halftone_line + 1) & 15 } else { halftone_line.wrapping_sub(1) & 15 };
        } else {
            x -= 1;
            r.dst_addr = r.dst_addr.wrapping_add(r.dst_x_inc as i32 as u32);
        }
    }
}

/// A blit worked out: what goes in each register, the source and destination as offsets from where their lines
/// start, and notes on anything to watch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub src_x_inc: i16,
    pub src_y_inc: i16,
    pub dst_x_inc: i16,
    pub dst_y_inc: i16,
    pub endmask: [u16; 3],
    pub x_count: u16,
    pub y_count: u16,
    pub skew: u8,
    pub fxsr: bool,
    pub nfsr: bool,
    /// Bytes from the start of the source's first line to the first word read.
    pub src_offset: u32,
    pub dst_offset: u32,
    /// Source words read each line.
    pub reads: u32,
    pub notes: Vec<String>,
}

impl Plan {
    /// $FF8A3D's value.
    pub fn skew_byte(&self) -> u8 {
        self.skew | if self.fxsr { 0x80 } else { 0 } | if self.nfsr { 0x40 } else { 0 }
    }
}

/// The end masks for a run of `width` pixels starting at x: the first word's, the middle's and the last word's.
pub fn endmasks(x: u32, width: u32) -> [u16; 3] {
    let last = x + width - 1;
    let first_mask = 0xFFFFu16 >> (x & 15);
    let last_mask = 0xFFFFu16 << (15 - (last & 15));
    if x / 16 == last / 16 { [first_mask & last_mask; 3] } else { [first_mask, 0xFFFF, last_mask] }
}

/// Works out a blit of `width` x `height` pixels from source x to destination x, going left to right and down.
/// `step` is the bytes from one word of a line to the next (2 for words side by side, 8 for one plane of the ST's
/// low resolution), and the strides the bytes from one line to the next.
pub fn plan(src_x: u32, dst_x: u32, width: u32, height: u32, step: u32, src_stride: u32, dst_stride: u32) -> Result<Plan, String> {
    if width == 0 || height == 0 {
        return Err(crate::i18n::tr("The width and height must be at least 1.").into());
    }
    if height > 65536 || step == 0 || step % 2 == 1 {
        return Err(crate::i18n::tr("The step between words must be even and not 0, and the height at most 65536.").into());
    }
    let (ss, ds) = (src_x & 15, dst_x & 15);
    let x_count = (dst_x + width - 1) / 16 - dst_x / 16 + 1;
    let src_words = (src_x + width - 1) / 16 - src_x / 16 + 1;
    let skew = ((ds + 16 - ss) & 15) as u8;
    let fxsr = ss > ds;
    let mut notes = Vec::new();
    // Reads a line: the words the line has, one more with FXSR, one fewer with NFSR.
    let mut nfsr = x_count + u32::from(fxsr) > src_words;
    if x_count == 1 && nfsr {
        // A one-word line with NFSR does something odd on the blitter; read a word more instead.
        nfsr = false;
        notes.push(crate::i18n::tr("Each line is one word, where NFSR misbehaves: the blitter reads one source word more than the pixels need, past their right.").into());
    }
    let reads = x_count + u32::from(fxsr) - u32::from(nfsr);
    let as_i16 = |v: i64, what: &str| -> Result<i16, String> { i16::try_from(v).map_err(|_| crate::i18n::tf!("The {what} ({v}) doesn't fit in the blitter's 16 bits.")) };
    let x_inc = as_i16(step as i64, "step")?;
    let plan = Plan {
        src_x_inc: x_inc,
        src_y_inc: as_i16(src_stride as i64 - (reads as i64 - 1) * step as i64, "source Y increment")?,
        dst_x_inc: x_inc,
        dst_y_inc: as_i16(dst_stride as i64 - (x_count as i64 - 1) * step as i64, "destination Y increment")?,
        endmask: endmasks(dst_x, width),
        x_count: u16::try_from(x_count).map_err(|_| crate::i18n::tr("The line is too wide.").to_string())?,
        y_count: (height % 65536) as u16,
        skew,
        fxsr,
        nfsr,
        src_offset: src_x / 16 * step,
        dst_offset: dst_x / 16 * step,
        reads,
        notes,
    };
    Ok(plan)
}

/// The register writes for a blit, as 68000 source: the blitter's base in an address register, then each register.
#[allow(clippy::too_many_arguments)]
pub fn source(plan: &Plan, hop_value: u8, op_value: u8, source_label: &str, dest_label: &str, register: &str, hog: bool) -> String {
    let mut out = String::new();
    let line = |out: &mut String, instruction: &str, comment: &str| out.push_str(&format!("        {instruction:<32}; {comment}\n"));
    line(&mut out, &format!("lea     $ffff8a00.w,{register}"), "the blitter");
    line(&mut out, &format!("move.w  #{},$20({register})", plan.src_x_inc), "SRCXINC");
    line(&mut out, &format!("move.w  #{},$22({register})", plan.src_y_inc), "SRCYINC");
    let place = |label: &str, offset: u32| if offset == 0 { label.to_string() } else { format!("{label}+{offset}") };
    line(&mut out, &format!("move.l  #{},$24({register})", place(source_label, plan.src_offset)), "SRCADDR");
    line(&mut out, &format!("move.w  #${:04X},$28({register})", plan.endmask[0]), "ENDMASK1");
    line(&mut out, &format!("move.w  #${:04X},$2a({register})", plan.endmask[1]), "ENDMASK2");
    line(&mut out, &format!("move.w  #${:04X},$2c({register})", plan.endmask[2]), "ENDMASK3");
    line(&mut out, &format!("move.w  #{},$2e({register})", plan.dst_x_inc), "DSTXINC");
    line(&mut out, &format!("move.w  #{},$30({register})", plan.dst_y_inc), "DSTYINC");
    line(&mut out, &format!("move.l  #{},$32({register})", place(dest_label, plan.dst_offset)), "DSTADDR");
    line(&mut out, &format!("move.w  #{},$36({register})", plan.x_count), "XCOUNT");
    line(&mut out, &format!("move.w  #{},$38({register})", plan.y_count), "YCOUNT");
    line(&mut out, &format!("move.b  #{},$3a({register})", hop_value & 3), &format!("HOP: {}", HOPS[(hop_value & 3) as usize]));
    line(&mut out, &format!("move.b  #{},$3b({register})", op_value & 15), &format!("OP: D = {}", OPS[(op_value & 15) as usize]));
    let mut skew = format!("SKEW {}", plan.skew);
    if plan.fxsr {
        skew.push_str(", FXSR");
    }
    if plan.nfsr {
        skew.push_str(", NFSR");
    }
    line(&mut out, &format!("move.b  #${:02X},$3d({register})", plan.skew_byte()), &skew);
    line(&mut out, &format!("move.b  #${:02X},$3c({register})", if hog { 0xC0 } else { 0x80 }), if hog { "start, in hog mode" } else { "start, sharing the bus" });
    out
}

/// The calculator's window.
pub struct Calculator {
    pub hop: u8,
    pub op: u8,
    pub src_x: u32,
    pub dst_x: u32,
    pub width: u32,
    pub height: u32,
    /// Bytes from one word of a line to the next.
    pub step: u32,
    pub src_stride: u32,
    pub dst_stride: u32,
    pub source_label: String,
    pub dest_label: String,
    pub register: String,
    pub hog: bool,
}

impl Default for Calculator {
    fn default() -> Self {
        Calculator { hop: 2, op: 3, src_x: 0, dst_x: 37, width: 64, height: 32, step: 8, src_stride: 160, dst_stride: 160, source_label: "sprite".into(), dest_label: "screen".into(), register: "a6".into(), hog: true }
    }
}

/// What the window asks the editor for.
pub enum Asked {
    Insert(String),
}

impl Calculator {
    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        ui.label(RichText::new(crate::i18n::tr("HOP: what the source is")).strong());
        ui.horizontal(|ui| {
            for (n, name) in HOPS.iter().enumerate() {
                ui.selectable_value(&mut self.hop, n as u8, crate::i18n::tf!("{n}: {name}"));
            }
        });
        ui.add_space(4.0);
        ui.label(RichText::new(crate::i18n::tr("OP: what the destination becomes")).strong());
        egui::Grid::new("st-blitter-ops").num_columns(4).spacing([6.0, 3.0]).show(ui, |ui| {
            for row in 0..4 {
                for column in 0..4 {
                    let n = (row * 4 + column) as u8;
                    ui.selectable_value(&mut self.op, n, RichText::new(crate::i18n::tf!("{n:>2}: {}", OPS[n as usize])).monospace().small());
                }
                ui.end_row();
            }
        });
        let (reads_s, reads_d) = op_reads(self.op);
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::i18n::tr("S D | result")).monospace());
            for (s, d) in [(0u16, 0u16), (0, 1), (1, 0), (1, 1)] {
                ui.label(RichText::new(crate::i18n::tf!("  {s} {d} | {}", op(self.op, s, d) & 1)).monospace());
            }
        });
        ui.label(crate::i18n::tf!("It reads {}.", match (reads_s, reads_d) {
            (true, true) => "the source and the destination",
            (true, false) => "the source only",
            (false, true) => "the destination only",
            (false, false) => "neither: it only writes",
        }));
        ui.separator();
        ui.label(RichText::new(crate::i18n::tr("The blit")).strong());
        egui::Grid::new("st-blitter-sizes").num_columns(4).show(ui, |ui| {
            ui.label(crate::i18n::tr("Source x"));
            ui.add(egui::DragValue::new(&mut self.src_x).range(0..=4095));
            ui.label(crate::i18n::tr("Destination x"));
            ui.add(egui::DragValue::new(&mut self.dst_x).range(0..=4095));
            ui.end_row();
            ui.label(crate::i18n::tr("Width (pixels)"));
            ui.add(egui::DragValue::new(&mut self.width).range(1..=4096));
            ui.label(crate::i18n::tr("Height (lines)"));
            ui.add(egui::DragValue::new(&mut self.height).range(1..=65536));
            ui.end_row();
            ui.label(crate::i18n::tr("Bytes between words"));
            egui::ComboBox::from_id_salt("st-blitter-step").selected_text(crate::i18n::tf!("{}", self.step)).show_ui(ui, |ui| {
                for (step, what) in [(2u32, "2: words side by side (one bitplane)"), (4, "4: medium resolution, one plane of two"), (8, "8: low resolution, one plane of four")] {
                    ui.selectable_value(&mut self.step, step, what);
                }
            });
            ui.label(crate::i18n::tr(""));
            ui.label(crate::i18n::tr(""));
            ui.end_row();
            ui.label(crate::i18n::tr("Source line bytes"));
            ui.add(egui::DragValue::new(&mut self.src_stride).range(2..=32766));
            ui.label(crate::i18n::tr("Destination line bytes"));
            ui.add(egui::DragValue::new(&mut self.dst_stride).range(2..=32766));
            ui.end_row();
        });
        match plan(self.src_x, self.dst_x, self.width, self.height, self.step, self.src_stride, self.dst_stride) {
            Err(problem) => {
                ui.label(problem);
            }
            Ok(plan) => {
                egui::Grid::new("st-blitter-plan").striped(true).num_columns(2).show(ui, |ui| {
                    let mut row = |name: &str, value: String| {
                        ui.label(RichText::new(name).monospace());
                        ui.label(RichText::new(value).monospace());
                        ui.end_row();
                    };
                    row("SKEW", format!("${:02X}: skew {}{}{}", plan.skew_byte(), plan.skew, if plan.fxsr { ", FXSR" } else { "" }, if plan.nfsr { ", NFSR" } else { "" }));
                    row("ENDMASK1-3", format!("%{:016b} ${:04X} ${:04X}", plan.endmask[0], plan.endmask[1], plan.endmask[2]));
                    row("XCOUNT, YCOUNT", format!("{}, {}", plan.x_count, plan.y_count));
                    row("SRCXINC, SRCYINC", format!("{}, {}", plan.src_x_inc, plan.src_y_inc));
                    row("DSTXINC, DSTYINC", format!("{}, {}", plan.dst_x_inc, plan.dst_y_inc));
                    row("Source starts", crate::i18n::tf!("{} bytes into its line ({} words read a line)", plan.src_offset, plan.reads));
                    row("Destination starts", crate::i18n::tf!("{} bytes into its line", plan.dst_offset));
                });
                for note in &plan.notes {
                    ui.label(RichText::new(note).small());
                }
                if !reads_s || self.hop & 2 == 0 {
                    ui.label(RichText::new(crate::i18n::tr("This operation doesn't read the source, so the skew and FXSR and NFSR don't matter.")).small());
                }
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Source"));
                    ui.add(egui::TextEdit::singleline(&mut self.source_label).desired_width(80.0).font(egui::TextStyle::Monospace));
                    ui.label(crate::i18n::tr("destination"));
                    ui.add(egui::TextEdit::singleline(&mut self.dest_label).desired_width(80.0).font(egui::TextStyle::Monospace));
                    ui.label(crate::i18n::tr("base in"));
                    ui.add(egui::TextEdit::singleline(&mut self.register).desired_width(30.0).font(egui::TextStyle::Monospace));
                    ui.checkbox(&mut self.hog, crate::i18n::tr("Hog mode"));
                });
                if ui.button(crate::i18n::tr("Put the register writes in at the cursor")).clicked() {
                    asked = Some(Asked::Insert(source(&plan, self.hop, self.op, &self.source_label, &self.dest_label, &self.register, self.hog)));
                }
            }
        }
        asked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hatari's 16 logic operations, as its blitter.c writes them out one by one.
    fn hatari_op(n: u8, s: u16, d: u16) -> u16 {
        match n {
            0 => 0,
            1 => s & d,
            2 => s & !d,
            3 => s,
            4 => !s & d,
            5 => d,
            6 => s ^ d,
            7 => s | d,
            8 => !s & !d,
            9 => !s ^ d,
            10 => !d,
            11 => s | !d,
            12 => !s,
            13 => !s | d,
            14 => !s | !d,
            _ => 0xFFFF,
        }
    }

    /// Which operations Hatari's table marks as reading the source and the destination.
    const HATARI_READS: [(bool, bool); 16] = [(false, false), (true, true), (true, true), (true, false), (true, true), (false, true), (true, true), (true, true), (true, true), (true, true), (false, true), (true, true), (true, false), (true, true), (true, true), (false, false)];

    #[test]
    fn the_logic_operations_are_hatari_s_truth_tables() {
        // Every bit combination: S %1100 against D %1010 covers all four, and then random words.
        for n in 0..16u8 {
            assert_eq!(op(n, 0b1100, 0b1010) & 15, hatari_op(n, 0b1100, 0b1010) & 15, "OP {n}");
            assert_eq!(op(n, 0b1100, 0b1010) & 15, ((n as u16 & 1) << 3 | (n as u16 & 2) << 1 | (n as u16 & 4) >> 1 | (n as u16 & 8) >> 3), "OP {n}'s bits, S=1 D=1 first");
            let mut seed = 0x1234_5678u32 ^ n as u32;
            for _ in 0..2000 {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let (s, d) = ((seed >> 16) as u16, seed as u16);
                assert_eq!(op(n, s, d), hatari_op(n, s, d), "OP {n} on ${s:04X} ${d:04X}");
            }
            assert_eq!(op_reads(n), HATARI_READS[n as usize], "OP {n} reads");
            for (s, d) in [(0u16, 0u16), (0, 0xFFFF), (0xFFFF, 0), (0xFFFF, 0xFFFF)] {
                assert_eq!(op(n, s, d), hatari_op(n, s, d));
            }
        }
        for h in 0..4u8 {
            for (s, t) in [(0x1234u16, 0xFF00u16), (0xFFFF, 0x0F0F)] {
                let want = match h {
                    0 => 0xFFFF,
                    1 => t,
                    2 => s,
                    _ => s & t,
                };
                assert_eq!(hop(h, s, t), want);
            }
        }
    }

    #[test]
    fn end_masks_cover_exactly_the_pixels() {
        for x in 0..48u32 {
            for width in 1..=64u32 {
                let masks = endmasks(x, width);
                let first = x / 16;
                let last = (x + width - 1) / 16;
                for word in first..=last {
                    let mask = if word == first { masks[0] } else if word == last { masks[2] } else { masks[1] };
                    for bit in 0..16 {
                        let pixel = word * 16 + bit;
                        let on = mask & (0x8000 >> bit) != 0;
                        assert_eq!(on, (x..x + width).contains(&pixel), "x {x} width {width} pixel {pixel}");
                    }
                }
            }
        }
    }

    fn pixel(memory: &[u8], line_start: usize, step: usize, x: usize) -> bool {
        let word = line_start + (x / 16) * step;
        let value = u16::from_be_bytes([memory[word], memory[word + 1]]);
        value & (0x8000 >> (x % 16)) != 0
    }

    /// Every source x and destination x within a word, every width to five words, in a single plane and in
    /// interleaved low-resolution planes: the blitter model, set up as the plan says, copies exactly the pixels.
    #[test]
    fn the_skew_and_fxsr_and_nfsr_rules_copy_exactly_the_pixels() {
        let mut checked = 0;
        for step in [2u32, 8] {
            let stride = 12 * step;
            let lines = 3;
            for src_off in 0..16u32 {
                for dst_off in 0..16u32 {
                    for width in 1..=72u32 {
                        let (src_x, dst_x) = (16 + src_off, 16 + dst_off);
                        let plan = plan(src_x, dst_x, width, lines, step, stride, stride).unwrap();
                        assert!(plan.reads > (src_x + width - 1) / 16 - src_x / 16, "enough words are read");
                        // Memory: source lines at 0, the destination well after; noise everywhere.
                        let mut memory = vec![0u8; 4096];
                        let mut seed = src_off * 7919 + dst_off * 104_729 + width;
                        for b in memory.iter_mut() {
                            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                            *b = (seed >> 16) as u8;
                        }
                        let before = memory.clone();
                        let dest_base = 2048u32;
                        let regs = Regs {
                            src_x_inc: plan.src_x_inc,
                            src_y_inc: plan.src_y_inc,
                            src_addr: plan.src_offset,
                            endmask: plan.endmask,
                            dst_x_inc: plan.dst_x_inc,
                            dst_y_inc: plan.dst_y_inc,
                            dst_addr: dest_base + plan.dst_offset,
                            x_count: plan.x_count,
                            y_count: plan.y_count,
                            hop: 2,
                            op: 3,
                            control: 0,
                            skew: plan.skew_byte(),
                            ..Default::default()
                        };
                        run(&regs, &mut memory);
                        for line in 0..lines as usize {
                            let (s, d) = (line * stride as usize, dest_base as usize + line * stride as usize);
                            for x in 0..(12 * 16) {
                                let want = if (dst_x..dst_x + width).contains(&(x as u32)) { pixel(&before, s, step as usize, x + src_x as usize - dst_x as usize) } else { pixel(&before, d, step as usize, x) };
                                assert_eq!(pixel(&memory, d, step as usize, x), want, "step {step}, source x {src_x}, destination x {dst_x}, width {width}, line {line}, pixel {x}: {plan:?}");
                            }
                        }
                        // Nothing outside the destination's words was touched (the other planes included).
                        for (i, (a, b)) in before.iter().zip(&memory).enumerate() {
                            let in_dest = i >= dest_base as usize && (i - dest_base as usize) % step as usize <= 1 && (i - dest_base as usize) / stride as usize <= lines as usize;
                            assert!(a == b || in_dest, "byte {i} changed");
                        }
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 2 * 16 * 16 * 72);
    }

    #[test]
    fn the_rules_are_the_familiar_ones() {
        // Same alignment: no skew, no extra reads.
        let p = plan(0, 0, 64, 1, 8, 160, 160).unwrap();
        assert_eq!((p.skew, p.fxsr, p.nfsr, p.x_count, p.endmask), (0, false, false, 4, [0xFFFF; 3]));
        assert_eq!((p.src_y_inc, p.dst_y_inc), (160 - 24, 160 - 24));
        // Moving right by 4: the destination spans one more word, which needs no source word of its own.
        let p = plan(0, 4, 32, 1, 8, 160, 160).unwrap();
        assert_eq!((p.skew, p.fxsr, p.nfsr, p.x_count, p.reads), (4, false, true, 3, 2));
        assert_eq!(p.endmask, [0x0FFF, 0xFFFF, 0xF000]);
        // Moving left: the first word needs two source words, read one extra first.
        let p = plan(4, 0, 32, 1, 8, 160, 160).unwrap();
        assert_eq!((p.skew, p.fxsr, p.nfsr, p.x_count, p.reads), (12, true, false, 2, 3));
        let text = source(&p, 2, 3, "sprite", "screen", "a6", true);
        assert!(text.contains("move.b  #$8C,$3d(a6)") && text.contains("move.l  #sprite,$24(a6)") && text.contains("move.b  #$C0,$3c(a6)"), "{text}");
        assert!(plan(0, 0, 0, 1, 8, 160, 160).is_err());
    }
}
