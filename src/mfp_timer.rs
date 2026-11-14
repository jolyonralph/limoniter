//! The MFP timer calculator: for a rate wanted from Timer A, B, C or D, the prescaler and count that come
//! nearest (the MFP's clock is 2.4576 MHz, divided by 4, 10, 16, 50, 64, 100 or 200, then counted down from 1 to
//! 256), the rate they really give and how far off it is; or Timer B counting display lines, an interrupt every N.
//! Then the register writes that set it up, at the cursor.

use crate::st_hw::{self, PRESCALERS};
use eframe::egui::{self, RichText, Ui};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timer {
    A,
    B,
    C,
    D,
}

impl Timer {
    pub const ALL: [Timer; 4] = [Timer::A, Timer::B, Timer::C, Timer::D];

    pub fn letter(self) -> char {
        match self {
            Timer::A => 'A',
            Timer::B => 'B',
            Timer::C => 'C',
            Timer::D => 'D',
        }
    }

    /// Its vector's address.
    pub fn vector(self) -> u32 {
        match self {
            Timer::A => 0x134,
            Timer::B => 0x120,
            Timer::C => 0x114,
            Timer::D => 0x110,
        }
    }

    /// Its data register.
    fn data(self) -> u32 {
        match self {
            Timer::A => 0xFFFA1F,
            Timer::B => 0xFFFA21,
            Timer::C => 0xFFFA23,
            Timer::D => 0xFFFA25,
        }
    }

    /// Its bit in the enable and mask registers: in A ($FFFA07, $FFFA13) or B ($FFFA09, $FFFA15).
    fn enable(self) -> (u32, u32, u8) {
        match self {
            Timer::A => (0xFFFA07, 0xFFFA13, 5),
            Timer::B => (0xFFFA07, 0xFFFA13, 0),
            Timer::C => (0xFFFA09, 0xFFFA15, 5),
            Timer::D => (0xFFFA09, 0xFFFA15, 4),
        }
    }
}

/// How the timer is to run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Want {
    /// Delay mode, at a rate in Hz.
    Rate(f64),
    /// Timer B in event count mode: an interrupt every so many display lines.
    Lines(u16),
}

/// A short address for the source: $FFFFFA1B.W.
fn short(address: u32) -> String {
    format!("$ffff{:04x}.w", address & 0xFFFF)
}

/// The writes that start a timer: stopped while it's set, its count, its mode, and its interrupt enabled and
/// unmasked; the handler's address goes in its vector first.
pub fn source(timer: Timer, control: u8, data: u8, handler: &str, said: &str) -> String {
    let mut out = String::new();
    let line = |out: &mut String, instruction: String, comment: &str| out.push_str(&format!("        {instruction:<32}; {comment}\n"));
    let letter = timer.letter();
    line(&mut out, format!("move.l  #{handler},${:x}.w", timer.vector()), &format!("Timer {letter}'s vector"));
    match timer {
        Timer::A | Timer::B => {
            let control_register = if timer == Timer::A { 0xFFFA19 } else { 0xFFFA1B };
            line(&mut out, format!("clr.b   {}", short(control_register)), &format!("stop Timer {letter} while it's set up"));
            line(&mut out, format!("move.b  #{data},{}", short(timer.data())), &format!("count {}", if data == 0 { 256 } else { data as u32 }));
            line(&mut out, format!("move.b  #{control},{}", short(control_register)), said);
        }
        Timer::C | Timer::D => {
            let (keep, shift) = if timer == Timer::C { (0x0F, 4) } else { (0xF0, 0) };
            line(&mut out, format!("and.b   #${keep:02x},{}", short(0xFFFA1D)), &format!("stop Timer {letter}, keeping the other"));
            line(&mut out, format!("move.b  #{data},{}", short(timer.data())), &format!("count {}", if data == 0 { 256 } else { data as u32 }));
            line(&mut out, format!("or.b    #${:02x},{}", control << shift, short(0xFFFA1D)), said);
        }
    }
    let (enable, mask, bit) = timer.enable();
    line(&mut out, format!("bset    #{bit},{}", short(enable)), &format!("Timer {letter}'s interrupt on"));
    line(&mut out, format!("bset    #{bit},{}", short(mask)), "and not masked");
    out
}

/// The calculator's window.
pub struct Calculator {
    pub timer: Timer,
    pub hz: f64,
    pub lines: u16,
    pub event: bool,
    pub handler: String,
    /// The setting chosen from those found, by its place in the list.
    pub chosen: usize,
}

impl Default for Calculator {
    fn default() -> Self {
        Calculator { timer: Timer::A, hz: 50.0, lines: 8, event: false, handler: "timer_handler".into(), chosen: 0 }
    }
}

pub enum Asked {
    Insert(String),
}

impl Calculator {
    pub fn want(&self) -> Want {
        if self.event && self.timer == Timer::B { Want::Lines(self.lines) } else { Want::Rate(self.hz) }
    }

    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        ui.horizontal(|ui| {
            ui.label("Timer");
            for timer in Timer::ALL {
                ui.selectable_value(&mut self.timer, timer, timer.letter().to_string());
            }
            ui.label(format!("(vector ${:03X})", self.timer.vector()));
        });
        if self.timer == Timer::B {
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.event, false, "At a rate");
                ui.radio_value(&mut self.event, true, "Every so many display lines");
            });
        }
        let found = match self.want() {
            Want::Lines(_) => {
                ui.horizontal(|ui| {
                    ui.label("An interrupt every");
                    ui.add(egui::DragValue::new(&mut self.lines).range(1..=256));
                    ui.label("lines");
                });
                ui.label(RichText::new("Event count mode counts the display's lines (its display-enable signal), so it only counts where the picture is; the first interrupt comes that many lines into the picture. Reload the count in the VBL handler each frame.").small());
                let data = if self.lines >= 256 { 0 } else { self.lines as u8 };
                vec![(8u8, data, 0.0, 0.0)]
            }
            Want::Rate(_) => {
                ui.horizontal(|ui| {
                    ui.label("Wanted rate");
                    ui.add(egui::DragValue::new(&mut self.hz).range(0.01..=614_400.0).speed(1.0).max_decimals(3));
                    ui.label("Hz");
                    for (hz, what) in [(50.0, "50 Hz"), (200.0, "200 Hz"), (12_517.0, "12.5 kHz"), (440.0, "A440")] {
                        if ui.small_button(what).clicked() {
                            self.hz = hz;
                            self.chosen = 0;
                        }
                    }
                });
                let found = st_hw::timer_settings(self.hz);
                if found.is_empty() {
                    ui.label("No setting gives that rate: the MFP's timers go from 48 Hz-ish (200 x 256) up to 614.4 kHz.");
                }
                egui::Grid::new("mfp-timer-settings").striped(true).num_columns(5).show(ui, |ui| {
                    ui.label(RichText::new("Control").strong());
                    ui.label(RichText::new("Divider").strong());
                    ui.label(RichText::new("Data").strong());
                    ui.label(RichText::new("Real rate").strong());
                    ui.label(RichText::new("Error").strong());
                    ui.end_row();
                    for (i, (control, data, rate, error)) in found.iter().enumerate() {
                        if ui.selectable_label(self.chosen == i, format!("{control}")).clicked() {
                            self.chosen = i;
                        }
                        ui.label(format!("{}", PRESCALERS[*control as usize - 1]));
                        ui.label(format!("{data} ({})", if *data == 0 { 256 } else { *data as u32 }));
                        ui.label(format!("{rate:.3} Hz"));
                        ui.label(format!("{error:+.3}%"));
                        ui.end_row();
                    }
                });
                found
            }
        };
        let (control, data, rate, error) = found.get(self.chosen.min(found.len().saturating_sub(1))).copied()?;
        let said = if control == 8 {
            format!("event count mode: every {} lines", if data == 0 { 256 } else { data as u32 })
        } else {
            format!("divide by {}: {rate:.3} Hz ({error:+.3}%)", PRESCALERS[control as usize - 1])
        };
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Handler");
            ui.add(egui::TextEdit::singleline(&mut self.handler).desired_width(120.0).font(egui::TextStyle::Monospace));
            if ui.button("Put the register writes in at the cursor").on_hover_text("In supervisor mode, with interrupts off (move.w #$2700,sr) while they're made").clicked() {
                asked = Some(Asked::Insert(source(self.timer, control, data, &self.handler, &said)));
            }
        });
        ui.label(RichText::new("The handler ends by clearing its in-service bit (TOS runs the MFP in software end-of-interrupt mode), then RTE.").small());
        asked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_writes_set_up_each_timer() {
        let a = source(Timer::A, 7, 246, "music", "divide by 200");
        assert!(a.contains("move.l  #music,$134.w") && a.contains("clr.b   $fffffa19.w") && a.contains("move.b  #246,$fffffa1f.w") && a.contains("move.b  #7,$fffffa19.w") && a.contains("bset    #5,$fffffa07.w") && a.contains("bset    #5,$fffffa13.w"), "{a}");
        let b = source(Timer::B, 8, 8, "raster", "every 8 lines");
        assert!(b.contains("$120.w") && b.contains("move.b  #8,$fffffa1b.w") && b.contains("bset    #0,$fffffa07.w"), "{b}");
        let c = source(Timer::C, 5, 192, "tick", "");
        assert!(c.contains("and.b   #$0f,$fffffa1d.w") && c.contains("or.b    #$50,$fffffa1d.w") && c.contains("bset    #5,$fffffa09.w"), "{c}");
        let d = source(Timer::D, 1, 2, "baud", "");
        assert!(d.contains("and.b   #$f0,$fffffa1d.w") && d.contains("or.b    #$01,$fffffa1d.w") && d.contains("bset    #4,$fffffa09.w"), "{d}");
        // It assembles.
        let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::M68k, std::path::Path::new("t.s"), &format!("{a}{c}music:\ntick:\n\trte\n"));
        assert!(assembly.assembly.ok(), "{:?}", assembly.assembly.problems);
    }

    #[test]
    fn the_calculator_wants_what_it_shows() {
        let mut calculator = Calculator::default();
        assert_eq!(calculator.want(), Want::Rate(50.0));
        calculator.timer = Timer::B;
        calculator.event = true;
        assert_eq!(calculator.want(), Want::Lines(8));
        calculator.timer = Timer::A;
        assert_eq!(calculator.want(), Want::Rate(50.0), "only Timer B counts lines");
    }
}
