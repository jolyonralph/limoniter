//! The raster interrupt helper: from a list of raster lines, each with a routine to call there, the 6502 code
//! (ACME syntax) that sets up a chain of raster interrupts, each one calling its routine and then setting up the
//! next, the last going back to the first. A line can be made stable, by the double interrupt: the first
//! interrupt, a line or two earlier, lets the next come in during NOPs, where it can only be a cycle late, and
//! a read of $D012 at the very end of the line takes that cycle away. A stable routine starts in cycle 4 of its
//! line, every frame, so a write to a colour register lands in the same place on the screen.
//!
//! With the KERNAL left in, the interrupts come through $0314, after the KERNAL's own 29 cycles of entry, and end
//! through $EA81. With it out ($01 = $35), they come straight through $FFFE and save the registers themselves.

use crate::c64_raster::Vic;
use eframe::egui::{self, RichText, Ui};
use std::fmt::Write;

/// One interrupt in the chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub line: u32,
    /// The routine it calls, which ends with RTS.
    pub routine: String,
    pub stable: bool,
}

/// What the code's made for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chain {
    pub vic: Vic,
    /// Whether the KERNAL stays in.
    pub kernal: bool,
    /// What the labels start with.
    pub prefix: String,
    pub entries: Vec<Entry>,
}

impl Default for Chain {
    fn default() -> Self {
        Chain {
            vic: Vic::Pal,
            kernal: false,
            prefix: "irq".into(),
            entries: vec![Entry { line: 0x10, routine: "frame".into(), stable: false }, Entry { line: 0xFC, routine: "bars".into(), stable: true }],
        }
    }
}

/// How many lines before a stable line the first of its two interrupts comes.
pub fn lead(kernal: bool) -> u32 {
    if kernal { 3 } else { 2 }
}

/// Code taking exactly so many cycles, with X as the counter: a DEX loop and NOPs (and a BIT of zero page for an
/// odd one). The loop must not cross a page, or its branches take a cycle more.
pub fn delay(cycles: u32) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut left = cycles;
    assert!(left >= 2 || left == 0, "no instruction takes a single cycle");
    // LDX #n then n times DEX and BNE (the last not taken) take 5n + 1.
    if left >= 13 {
        let mut n = (left - 1) / 5;
        while n > 1 && (left - (5 * n + 1) == 1) {
            n -= 1;
        }
        out.push((format!("ldx #{n}"), format!("{} cycles of waiting", 5 * n + 1)));
        out.push(("dex".into(), String::new()));
        out.push(("bne *-1".into(), String::new()));
        left -= 5 * n + 1;
    }
    if left % 2 == 1 {
        out.push(("bit $ea".into(), "3 cycles".into()));
        left -= 3;
    }
    for _ in 0..left / 2 {
        out.push(("nop".into(), String::new()));
    }
    out
}

impl Chain {
    /// The code, or why it can't be made.
    pub fn source(&self) -> Result<String, String> {
        let p = self.prefix.trim();
        if p.is_empty() || !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || p.starts_with(|c: char| c.is_ascii_digit()) {
            return Err(crate::i18n::tr("The labels' prefix has to be a word of letters, digits and _.").into());
        }
        if self.entries.is_empty() {
            return Err(crate::i18n::tr("Add a line to interrupt at.").into());
        }
        let lines = self.vic.lines();
        let mut entries = self.entries.clone();
        entries.sort_by_key(|e| e.line);
        for (i, e) in entries.iter().enumerate() {
            if e.routine.trim().is_empty() {
                return Err(crate::i18n::tf!("Line {} needs a routine to call.", e.line));
            }
            if e.line >= lines {
                return Err(crate::i18n::tf!("Line {} isn't there: a {} frame has lines 0 to {}.", e.line, self.vic.short(), lines - 1));
            }
            if i > 0 && entries[i - 1].line == e.line {
                return Err(crate::i18n::tf!("Line {} is in the list twice.", e.line));
            }
            if e.stable {
                let first = e.line as i64 - lead(self.kernal) as i64;
                if first < 1 || (first as u32 >> 8) != (e.line >> 8) {
                    return Err(crate::i18n::tf!("A stable interrupt's first half comes {} lines before line {}, which is too near line 0 or 256 for it; pick another line.", lead(self.kernal), e.line));
                }
            }
        }
        let high = entries.iter().any(|e| e.line >= 256);
        let vector = if self.kernal { "$0314" } else { "$fffe" };
        let mut out = String::new();
        let line = |out: &mut String, label: &str, code: &str, comment: &str| {
            let _ = match (label.is_empty(), comment.is_empty()) {
                (true, true) => writeln!(out, "        {code}"),
                (true, false) => writeln!(out, "        {code:<24}; {comment}"),
                (false, true) => writeln!(out, "{label}\n        {code}"),
                (false, false) => writeln!(out, "{label}\n        {code:<24}; {comment}"),
            };
        };
        // The interrupt each entry starts with, and the line it comes on.
        let first_label = |i: usize| format!("{p}_{}", i + 1);
        let first_line = |e: &Entry| if e.stable { e.line - lead(self.kernal) } else { e.line };
        let _ = writeln!(
            out,
            "; Raster interrupts for {} ({} cycles a line), {}. {p}_setup starts them; each\n; routine is called with JSR and ends with RTS. Stable ones start in cycle 4 of their line: keep\n; bad lines and sprites off the {} lines before them.",
            self.vic.short(),
            self.vic.cycles(),
            if self.kernal { "the KERNAL left in (through $0314)" } else { "the KERNAL switched out (through $FFFE)" },
            lead(self.kernal)
        );
        let _ = writeln!(out, "{p}_setup");
        line(&mut out, "", "sei", "");
        line(&mut out, "", "lda #$7f", "");
        line(&mut out, "", "sta $dc0d", "no CIA 1 interrupts (the KERNAL's timer)");
        line(&mut out, "", "sta $dd0d", "nor CIA 2's NMIs");
        line(&mut out, "", "lda $dc0d", "reading them clears any waiting");
        line(&mut out, "", "lda $dd0d", "");
        if !self.kernal {
            line(&mut out, "", "lda #$35", "");
            line(&mut out, "", "sta $01", "KERNAL and BASIC out, I/O in");
            line(&mut out, "", &format!("lda #<{p}_nmi"), "");
            line(&mut out, "", "sta $fffa", "");
            line(&mut out, "", &format!("lda #>{p}_nmi"), "");
            line(&mut out, "", "sta $fffb", "RESTORE does nothing");
        }
        line(&mut out, "", &format!("lda #<{}", first_label(0)), "");
        line(&mut out, "", &format!("sta {vector}"), "");
        line(&mut out, "", &format!("lda #>{}", first_label(0)), "");
        line(&mut out, "", &format!("sta {}", if self.kernal { "$0315" } else { "$ffff" }), "");
        let first = first_line(&entries[0]);
        line(&mut out, "", &format!("lda #{}", first & 0xFF), &format!("the first at line {first}"));
        line(&mut out, "", "sta $d012", "");
        line(&mut out, "", "lda $d011", "");
        line(&mut out, "", "and #$7f", "");
        if first >= 256 {
            line(&mut out, "", "ora #$80", "line 256 and on");
        }
        line(&mut out, "", "sta $d011", "");
        line(&mut out, "", "lda #$01", "");
        line(&mut out, "", "sta $d01a", "raster interrupts on");
        line(&mut out, "", "sta $d019", "and none waiting");
        line(&mut out, "", "cli", "");
        line(&mut out, "", "rts", "");
        if !self.kernal {
            let _ = writeln!(out, "{p}_nmi");
            line(&mut out, "", "rti", "");
        }
        out.push('\n');
        // The code that runs between the chain's entries must not cross a page in its timed loops.
        let _ = writeln!(out, "        !align 255, 0           ; timed loops mustn't cross a page");
        for (i, e) in entries.iter().enumerate() {
            let next = &entries[(i + 1) % entries.len()];
            let label = first_label(i);
            if e.stable {
                let second = format!("{label}_stable");
                let _ = writeln!(out, "; Line {}: {}, stable from cycle 4 of line {}.", first_line(e), e.routine.trim(), e.line);
                let _ = writeln!(out, "{label}");
                if self.kernal {
                    line(&mut out, "", "tsx", "the stack as it is now, to come back to");
                    line(&mut out, "", &format!("stx {second}+1"), "");
                } else {
                    line(&mut out, "", "pha", "");
                    line(&mut out, "", "txa", "");
                    line(&mut out, "", "pha", "");
                }
                line(&mut out, "", &format!("lda #<{second}"), "");
                line(&mut out, "", &format!("sta {vector}"), "");
                line(&mut out, "", &format!("lda #>{second}"), "");
                line(&mut out, "", &format!("sta {}", if self.kernal { "$0315" } else { "$ffff" }), "");
                line(&mut out, "", "inc $d012", if self.kernal { "two lines on" } else { "the next line" });
                if self.kernal {
                    line(&mut out, "", "inc $d012", "");
                }
                line(&mut out, "", "lda #$01", "");
                line(&mut out, "", "sta $d019", "this one's done");
                if !self.kernal {
                    line(&mut out, "", "tsx", "the stack as it is now, to come back to");
                }
                line(&mut out, "", "cli", "");
                let nops = if self.kernal { 32 } else { 12 };
                let _ = writeln!(out, "        ; The second interrupt comes during these, a cycle late at most.");
                for chunk in 0..nops / 8 + usize::from(nops % 8 != 0) {
                    let n = (nops - chunk * 8).min(8);
                    let _ = writeln!(out, "        {}", vec!["nop"; n].join(" : "));
                }
                let _ = writeln!(out, "{second}");
                if self.kernal {
                    line(&mut out, "", "ldx #0", "where the stack was (written above)");
                }
                line(&mut out, "", "txs", "the second interrupt's return isn't wanted");
                // Through the KERNAL it starts 29 cycles later; and the LDX takes 2 more.
                let spare = self.vic.cycles() - 63;
                let wait = if self.kernal { 13 } else { 44 } + spare;
                for (code, comment) in delay(wait) {
                    line(&mut out, "", &code, &comment);
                }
                line(&mut out, "", "lda $d012", "");
                line(&mut out, "", "cmp $d012", "read in the line's last cycle, or the next line's first");
                line(&mut out, "", "beq *+2", "a cycle more if it's still the same line: stable now");
                if !self.kernal {
                    line(&mut out, "", "tya", "");
                    line(&mut out, "", "pha", "");
                }
            } else {
                let _ = writeln!(out, "; Line {}: {}.", e.line, e.routine.trim());
                let _ = writeln!(out, "{label}");
                if !self.kernal {
                    line(&mut out, "", "pha", "");
                    line(&mut out, "", "txa", "");
                    line(&mut out, "", "pha", "");
                    line(&mut out, "", "tya", "");
                    line(&mut out, "", "pha", "");
                }
            }
            line(&mut out, "", &format!("jsr {}", e.routine.trim()), "");
            let next_line = first_line(next);
            line(&mut out, "", &format!("lda #<{}", first_label((i + 1) % entries.len())), "");
            line(&mut out, "", &format!("sta {vector}"), "");
            line(&mut out, "", &format!("lda #>{}", first_label((i + 1) % entries.len())), "");
            line(&mut out, "", &format!("sta {}", if self.kernal { "$0315" } else { "$ffff" }), "");
            line(&mut out, "", &format!("lda #{}", next_line & 0xFF), &format!("next, line {next_line}"));
            line(&mut out, "", "sta $d012", "");
            if high {
                line(&mut out, "", "lda $d011", "");
                line(&mut out, "", "and #$7f", "");
                if next_line >= 256 {
                    line(&mut out, "", "ora #$80", "");
                }
                line(&mut out, "", "sta $d011", "");
            }
            line(&mut out, "", "lda #$01", "");
            line(&mut out, "", "sta $d019", "this one's done");
            if self.kernal {
                line(&mut out, "", "jmp $ea81", "the KERNAL's end: registers back, RTI");
            } else {
                line(&mut out, "", "pla", "");
                line(&mut out, "", "tay", "");
                line(&mut out, "", "pla", "");
                line(&mut out, "", "tax", "");
                line(&mut out, "", "pla", "");
                line(&mut out, "", "rti", "");
            }
            out.push('\n');
        }
        Ok(out)
    }
}

/// The helper's window.
#[derive(Default)]
pub struct Helper {
    pub chain: Chain,
}

pub enum Asked {
    Insert(String),
}

impl Helper {
    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        let c = &mut self.chain;
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Machine"));
            for vic in Vic::ALL {
                ui.selectable_value(&mut c.vic, vic, crate::i18n::tr(vic.name()));
            }
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut c.kernal, false, crate::i18n::tr("KERNAL out ($FFFE)"));
            ui.radio_value(&mut c.kernal, true, crate::i18n::tr("KERNAL in ($0314)"));
            ui.separator();
            ui.label(crate::i18n::tr("Labels start"));
            ui.add(egui::TextEdit::singleline(&mut c.prefix).desired_width(80.0));
        });
        ui.label(RichText::new(crate::i18n::tr("Each line calls its routine (ending with RTS). A stable one starts in cycle 4 of its line, the same every frame: for raster bars, splits and opening the borders.")).small());
        let mut remove = None;
        let last = c.vic.lines() - 1;
        egui::Grid::new("irq chain").num_columns(4).show(ui, |ui| {
            ui.label(crate::i18n::tr("Line"));
            ui.label(crate::i18n::tr("Routine"));
            ui.label(crate::i18n::tr("Stable"));
            ui.label(crate::i18n::tr(""));
            ui.end_row();
            for (i, e) in c.entries.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut e.line).range(0..=last));
                    ui.label(RichText::new(crate::i18n::tf!("${:03X}", e.line)).small());
                });
                ui.add_sized([130.0, 20.0], egui::TextEdit::singleline(&mut e.routine));
                ui.checkbox(&mut e.stable, crate::i18n::tr(""));
                if ui.small_button(crate::i18n::tr("Remove")).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            c.entries.remove(i);
        }
        if ui.button(crate::i18n::tr("Add a line")).clicked() {
            let line = c.entries.iter().map(|e| e.line).max().map_or(0x10, |l| (l + 16).min(last));
            c.entries.push(Entry { line, routine: format!("routine{}", c.entries.len() + 1), stable: false });
        }
        // Warnings: a stable line whose lead-in lines are bad lines, with the usual scroll.
        for e in c.entries.iter().filter(|e| e.stable) {
            let lead = lead(c.kernal);
            if (e.line.saturating_sub(lead)..=e.line).any(|l| crate::c64_raster::is_bad_line(l, 3, true)) {
                ui.label(RichText::new(crate::i18n::tf!("Line {} or the {lead} before it is a bad line with the usual scroll (3): the timing won't hold there.", e.line)).color(ui.visuals().warn_fg_color));
            }
        }
        ui.add_space(4.0);
        match c.source() {
            Ok(text) => {
                if ui.button(crate::i18n::tr("Put the code at the cursor")).clicked() {
                    asked = Some(Asked::Insert(text));
                }
            }
            Err(why) => {
                ui.colored_label(ui.visuals().error_fg_color, why);
            }
        }
        asked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assemblers::{Cpu, assemble_own};
    use std::path::Path;

    #[test]
    fn delays_take_the_cycles_asked() {
        for cycles in (2..=200).filter(|c| *c != 1) {
            let code: String = delay(cycles).iter().map(|(c, _)| format!(" {c}\n")).collect();
            let text = format!("*=$1000\n{code}");
            let a = assemble_own(Cpu::M6502, Path::new("d.a"), &text).assembly;
            assert!(a.ok(), "{:?}", a.problems);
            // The block counts each line once; the loop (LDX, DEX, BNE: 6 cycles as lines) runs for 5n + 1.
            let n = delay(cycles).first().and_then(|(c, _)| c.strip_prefix("ldx #")).map_or(0, |n| n.parse::<u32>().unwrap());
            let (sum, _) = crate::m6502_timing::block(&a, 0, 0, 100);
            let looped = if n > 0 { sum.least - 6 + 5 * n + 1 } else { sum.least };
            assert_eq!(looped, cycles, "{cycles}: {code}");
        }
    }

    #[test]
    fn the_chain_assembles_both_ways() {
        for kernal in [false, true] {
            for vic in Vic::ALL {
                let chain = Chain { kernal, vic, ..Default::default() };
                let text = chain.source().unwrap();
                let program = format!("*=$c000\n{text}frame\n rts\nbars\n rts\n");
                let a = assemble_own(Cpu::M6502, Path::new("c.a"), &program).assembly;
                assert!(a.ok(), "{:?}\n{program}", a.problems);
                assert!(text.contains(if kernal { "jmp $ea81" } else { "rti" }));
            }
        }
        // Lines from 256 on set $D011's top bit.
        let chain = Chain { entries: vec![Entry { line: 0x10, routine: "a".into(), stable: false }, Entry { line: 300, routine: "b".into(), stable: false }], ..Default::default() };
        let text = chain.source().unwrap();
        assert!(text.contains("lda #44                 ; next, line 300\n        sta $d012\n        lda $d011\n        and #$7f\n        ora #$80"), "{text}");
        // Too near line 0.
        let chain = Chain { entries: vec![Entry { line: 1, routine: "a".into(), stable: true }], ..Default::default() };
        assert!(chain.source().is_err());
        let chain = Chain { prefix: "9x".into(), ..Default::default() };
        assert!(chain.source().is_err());
    }

    /// A program whose stable routine, at line 252, draws 16 lines with white in the middle of the border, a line
    /// 63, 65 or 64 cycles long; its main loop runs instructions of different lengths, so the first interrupt
    /// comes at a different point of one each frame.
    fn probe(kernal: bool, vic: Vic, correct: bool) -> String {
        let chain = Chain { kernal, vic, prefix: "irq".into(), entries: vec![Entry { line: 0x10, routine: "frame".into(), stable: false }, Entry { line: 0xFC, routine: "bars".into(), stable: true }] };
        let mut text = chain.source().unwrap();
        if !correct {
            text = text.replace("beq *+2 ", "bit $ea ");
        }
        let (count, more) = match vic {
            Vic::Pal => (5, ""),
            Vic::Ntsc => (5, " nop\n"),
            Vic::OldNtsc => (4, " bit $ea\n bit $ea\n"),
        };
        format!(
            "* = $0801\n !word +, 10\n !byte $9e\n !text \"2061\"\n !byte 0\n+ !word 0\nstart\n jsr irq_setup\n lda #$7f\n sta $dc00\nmain\n inc $0401\n lda $dc01\n and #$10\n bne main\n jmp main\nframe\n rts\n{text}\n !align 255, 0\nbars\n ldx #16\nbl\n lda #1\n sta $d020\n{}\n lda #0\n sta $d020\n ldy #{count}\n dey\n bne *-1\n{more} dex\n bne bl\n rts\n",
            " nop\n".repeat(10).trim_end()
        )
    }

    /// Where white starts on each row of a screenshot that has white.
    fn white_starts(png: &[u8]) -> Vec<usize> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().unwrap();
        let mut data = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut data).unwrap();
        let step = if frame.color_type == png::ColorType::Rgba { 4 } else { 3 };
        let width = frame.width as usize;
        data[..frame.buffer_size()].chunks(width * step).filter_map(|row| row.chunks(step).position(|p| p[..3] == [255, 255, 255])).collect()
    }

    /// A real VICE, where LIMONITER_VICE says where x64sc is (and LIMONITER_VICE_OPTIONS anything it needs, such as
    /// where its ROMs are): on PAL, NTSC and old NTSC, the KERNAL in and out, a stable routine's white starts in
    /// the same place on every line of every frame looked at; without the last step, the cycle it takes away
    /// shows. It opens VICE's window: run it under xvfb-run where there's no screen.
    #[test]
    fn stable_interrupts_hold_in_a_real_vice() {
        let Some(vice) = std::env::var_os("LIMONITER_VICE") else {
            return;
        };
        let options = std::env::var("LIMONITER_VICE_OPTIONS").unwrap_or_default();
        let dir = std::env::temp_dir().join(format!("limoniter-stable-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut moved_without = 0;
        for vic in Vic::ALL {
            for kernal in [false, true] {
                let model = match vic {
                    Vic::Pal => "c64",
                    Vic::Ntsc => "ntsc",
                    Vic::OldNtsc => "oldntsc",
                };
                let mut starts = Vec::new();
                for correct in [true, false] {
                    let a = assemble_own(Cpu::M6502, Path::new("p.a"), &probe(kernal, vic, correct)).assembly;
                    assert!(a.ok(), "{:?}", a.problems);
                    let prg = dir.join("probe.prg");
                    std::fs::write(&prg, crate::asm::output::commodore_program(&a).unwrap()).unwrap();
                    let mut seen = std::collections::BTreeSet::new();
                    for shot in 0..6u32 {
                        let png = dir.join("shot.png");
                        let _ = std::fs::remove_file(&png);
                        let status = std::process::Command::new(&vice)
                            .arg("-default")
                            .args(["-model", model])
                            .args(options.split_whitespace())
                            .args(["-sounddev", "dummy", "-warp", "-autostartprgmode", "1", "-limitcycles"])
                            .arg((4_000_000 + shot * 123_457).to_string())
                            .arg("-exitscreenshot")
                            .arg(&png)
                            .arg("-autostart")
                            .arg(&prg)
                            .output()
                            .unwrap();
                        let picture = std::fs::read(&png).unwrap_or_else(|_| panic!("no screenshot: {}", String::from_utf8_lossy(&status.stdout)));
                        let rows = white_starts(&picture);
                        assert_eq!(rows.len(), 16, "{vic:?}, KERNAL {kernal}: 16 lines with white");
                        seen.extend(rows);
                    }
                    starts.push(seen);
                }
                assert_eq!(starts[0].len(), 1, "{vic:?}, KERNAL {kernal}: stable: {:?}", starts[0]);
                if starts[1] != starts[0] {
                    moved_without += 1;
                }
            }
        }
        // Without the last step the routine starts a cycle late in some of them (8 pixels).
        assert!(moved_without >= 3, "{moved_without}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
