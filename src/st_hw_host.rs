//! The Atari ST windows: the hardware guide (the registers of the ST, STE, Mega STE, TT and Falcon, searchable,
//! grouped by chip and filtered by machine, with any value read into its bits and a colour shown as the ST and the
//! STE show it) with the TOS calls beside it; the STE blitter calculator; and the MFP timer calculator. Each is a
//! window kept open beside the text, putting what it makes at the cursor.

use super::*;
use crate::st_hw::{self, Chip, Machine, Register};
use crate::st_tos::{self, Function, Trap};
use egui::{Color32, RichText, Sense, vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Registers,
    Calls,
}

/// The hardware window's state while it's open.
pub(super) struct HwWindow {
    pub tab: Tab,
    search: String,
    machine: Option<Machine>,
    /// The register being looked at, by address (and for a run, which of it).
    selected: u32,
    value: String,
    /// For the YM's data register: which YM register the value is for.
    ym_register: u8,
    call_search: String,
    /// The TOS function being looked at.
    call: Option<(Trap, u16)>,
}

impl Default for HwWindow {
    fn default() -> Self {
        HwWindow { tab: Tab::Registers, search: String::new(), machine: None, selected: 0xFF8240, value: String::new(), ym_register: 7, call_search: String::new(), call: Some((Trap::Gemdos, 0x20)) }
    }
}

/// The Atari ST windows, while they're open.
#[derive(Default)]
pub(super) struct StTools {
    pub hardware: Option<Box<HwWindow>>,
    pub blitter: Option<Box<crate::st_blitter::Calculator>>,
    pub timers: Option<Box<crate::mfp_timer::Calculator>>,
}

/// A number typed in: $hex, %binary, 0x hex or decimal.
fn number(text: &str) -> Option<i64> {
    crate::amiga_source::evaluate(text.trim().trim_start_matches('#'), &|_| None)
}

/// Whether a register goes with what's been searched for: its name, its address, or words of what it does.
fn matches(register: &Register, search: &str) -> bool {
    let search = search.trim().to_ascii_lowercase();
    if search.is_empty() {
        return true;
    }
    if let Ok(n) = u32::from_str_radix(search.trim_start_matches('$').trim_start_matches("0x").trim_end_matches(".w"), 16)
        && (register.holds(n & 0xFF_FFFF) || register.holds(0xFF_0000 | (n & 0xFFFF)))
    {
        return true;
    }
    register.name.to_ascii_lowercase().contains(&search)
        || register.about.to_ascii_lowercase().contains(&search)
        || register.fields.iter().any(|f| f.name.to_ascii_lowercase().contains(&search) || f.about.to_ascii_lowercase().contains(&search))
}

fn call_matches(function: &Function, search: &str) -> bool {
    let search = search.trim().to_ascii_lowercase();
    search.is_empty()
        || function.name.to_ascii_lowercase().contains(&search)
        || function.about.to_ascii_lowercase().contains(&search)
        || number(&search).is_some_and(|n| n == function.number as i64)
}

fn swatch(ui: &mut egui::Ui, rgb: [u8; 3], tip: &str) {
    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 14.0), Sense::hover());
    ui.painter().rect_filled(rect, 2.0, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
    ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, ui.visuals().weak_text_color()), egui::StrokeKind::Inside);
    response.on_hover_text(tip);
}

/// A call to a function as source: its arguments pushed last first, the function's number, the trap, and the
/// stack put right.
pub(super) fn call_source(function: &Function) -> String {
    let mut out = String::new();
    let line = |out: &mut String, instruction: String, comment: String| out.push_str(&format!("        {instruction:<32}; {comment}\n"));
    for arg in function.args.iter().rev() {
        line(&mut out, format!("move.{}  #0,-(sp)", if arg.long { "l" } else { "w" }), format!("{}: {}", arg.name, arg.about));
    }
    line(&mut out, format!("move.w  #${:02x},-(sp)", function.number), function.name.to_string());
    line(&mut out, format!("trap    #{}", function.trap.number()), format!("{}: {}", function.trap.name(), function.returns));
    let bytes = function.stack_bytes();
    let tidy = if bytes <= 8 { format!("addq.l  #{bytes},sp") } else { format!("lea     {bytes}(sp),sp") };
    line(&mut out, tidy, "take the arguments off".into());
    out
}

impl HwWindow {
    fn registers_tab(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let mut insert = None;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search: name, address or words").desired_width(240.0));
            egui::ComboBox::from_id_salt("st-hw-machine").selected_text(self.machine.map_or("Every machine", |m| m.name())).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.machine, None, "Every machine");
                for machine in Machine::ALL {
                    ui.selectable_value(&mut self.machine, Some(machine), format!("What the {} has", machine.name()));
                }
            });
        });
        ui.separator();
        let searching = !self.search.trim().is_empty();
        ui.horizontal_top(|ui| {
            egui::ScrollArea::vertical().id_salt("st-hw-list").max_height(440.0).min_scrolled_height(440.0).show(ui, |ui| {
                ui.set_width(260.0);
                for chip in Chip::ALL {
                    let shown: Vec<&Register> = st_hw::registers().iter().filter(|r| r.chip == chip && self.machine.is_none_or(|m| r.machines & m.bit() != 0) && matches(r, &self.search)).collect();
                    if shown.is_empty() {
                        continue;
                    }
                    let header = egui::CollapsingHeader::new(format!("{} ({})", chip.name(), shown.len())).id_salt(("st-hw-chip", chip as u8));
                    let header = if searching { header.open(Some(true)) } else { header.default_open(chip == Chip::Video) };
                    header.show(ui, |ui| {
                        for register in shown {
                            let label = format!("${:06X}  {}", register.address, register.name);
                            if ui.selectable_label(register.holds(self.selected), RichText::new(label).monospace()).on_hover_text(&register.about).clicked() {
                                self.selected = register.address;
                            }
                        }
                    });
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(480.0);
                let Some(register) = st_hw::at(self.selected) else {
                    ui.label("Choose a register.");
                    return;
                };
                ui.label(RichText::new(&register.name).strong().size(16.0));
                ui.label(RichText::new(register.heading()).monospace().small());
                ui.add_space(4.0);
                ui.label(&register.about);
                ui.add_space(6.0);
                if !register.fields.is_empty() {
                    egui::Grid::new("st-hw-bits").striped(true).num_columns(3).show(ui, |ui| {
                        for f in register.fields {
                            ui.label(RichText::new(f.bits()).monospace());
                            ui.label(RichText::new(f.name).monospace().strong());
                            ui.add(egui::Label::new(f.about).wrap());
                            ui.end_row();
                        }
                    });
                    ui.add_space(6.0);
                }
                if !st_hw::status_fields(register).is_empty() {
                    ui.label(RichText::new("Read, it's the status:").strong());
                    egui::Grid::new("st-hw-status").striped(true).num_columns(3).show(ui, |ui| {
                        for f in st_hw::status_fields(register) {
                            ui.label(RichText::new(f.bits()).monospace());
                            ui.label(RichText::new(f.name).monospace().strong());
                            ui.add(egui::Label::new(f.about).wrap());
                            ui.end_row();
                        }
                    });
                    ui.add_space(6.0);
                }
                if register.count > 1 {
                    ui.horizontal(|ui| {
                        ui.label("Which:");
                        let mut index = register.index(self.selected);
                        if ui.add(egui::DragValue::new(&mut index).range(0..=register.count - 1)).changed() {
                            self.selected = register.address + index * register.size.bytes();
                        }
                        ui.label(RichText::new(format!("${:06X}", self.selected)).monospace());
                    });
                }
                if register.address == 0xFF8802 {
                    ui.horizontal(|ui| {
                        ui.label("YM register:");
                        egui::ComboBox::from_id_salt("st-hw-ym").selected_text(format!("{}: {}", self.ym_register, st_hw::YM[self.ym_register as usize].name)).show_ui(ui, |ui| {
                            for r in &st_hw::YM {
                                ui.selectable_value(&mut self.ym_register, r.number, format!("{}: {}", r.number, r.name));
                            }
                        });
                    });
                    ui.label(RichText::new(st_hw::YM[self.ym_register as usize].about).small());
                }
                ui.horizontal(|ui| {
                    ui.label("Value:");
                    ui.add(egui::TextEdit::singleline(&mut self.value).hint_text("$0777").desired_width(100.0).font(egui::TextStyle::Monospace));
                    if ui.button("Copy name").clicked() {
                        ui.ctx().copy_text(register.name.clone());
                    }
                    if ui.button("Copy the chip's EQUs").on_hover_text(format!("Every register of {} as an EQU line", register.chip.name())).clicked() {
                        ui.ctx().copy_text(st_hw::equates(Some(register.chip)));
                    }
                    if ui.button("Put them in").on_hover_text("The EQU lines at the cursor").clicked() {
                        insert = Some(st_hw::equates(Some(register.chip)));
                    }
                });
                if let Some(value) = number(&self.value) {
                    let value = value as u32;
                    if register.address == 0xFF8802 {
                        ui.label(RichText::new(format!("YM register {} = ${:02X}: {}", self.ym_register, value as u8, st_hw::explain_ym(self.ym_register, value as u8))).strong());
                    } else if let Some(n) = register.st_colour() {
                        let word = value as u16;
                        ui.label(RichText::new(format!("Colour {n} = ${word:03X}")).strong());
                        ui.horizontal(|ui| {
                            swatch(ui, st_hw::st_rgb(word), "On an ST");
                            let [r, g, b] = [(word >> 8) & 7, (word >> 4) & 7, word & 7];
                            ui.label(format!("ST: red {r}, green {g}, blue {b} (of 7)"));
                        });
                        ui.horizontal(|ui| {
                            swatch(ui, st_hw::ste_rgb(word), "On an STE");
                            let [r, g, b] = st_hw::ste_levels(word);
                            ui.label(format!("STE: red {r}, green {g}, blue {b} (of 15)"));
                        });
                    } else {
                        let said = st_hw::describe_value(register, self.selected, value);
                        for (i, line) in said.lines().skip(1).enumerate() {
                            let text = RichText::new(line.replace('\u{25A0}', ""));
                            ui.label(if i == 0 { text.strong() } else { text.monospace().small() });
                        }
                    }
                } else if !self.value.trim().is_empty() {
                    ui.label("That isn't a number: write $0777, %0111 or 1911.");
                }
            });
        });
        insert
    }

    fn calls_tab(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let mut insert = None;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.call_search).hint_text("Search: name, number or words").desired_width(240.0));
            if ui.button("Copy every call as EQUs").on_hover_text("Cconws equ 9 and the rest, to push the numbers by name").clicked() {
                ui.ctx().copy_text(st_tos::FUNCTIONS.iter().map(|f| f.equate() + "\n").collect::<String>());
            }
        });
        ui.separator();
        ui.horizontal_top(|ui| {
            egui::ScrollArea::vertical().id_salt("st-calls-list").max_height(440.0).min_scrolled_height(440.0).show(ui, |ui| {
                ui.set_width(260.0);
                let searching = !self.call_search.trim().is_empty();
                for trap in [Trap::Gemdos, Trap::Bios, Trap::Xbios] {
                    let shown: Vec<&Function> = st_tos::FUNCTIONS.iter().filter(|f| f.trap == trap && call_matches(f, &self.call_search)).collect();
                    if shown.is_empty() {
                        continue;
                    }
                    let header = egui::CollapsingHeader::new(format!("{} (trap #{}, {})", trap.name(), trap.number(), shown.len())).id_salt(("st-calls", trap.number()));
                    let header = if searching { header.open(Some(true)) } else { header.default_open(trap == Trap::Gemdos) };
                    header.show(ui, |ui| {
                        for function in shown {
                            let chosen = self.call == Some((function.trap, function.number));
                            if ui.selectable_label(chosen, RichText::new(format!("${:02X}  {}", function.number, function.name)).monospace()).on_hover_text(function.about).clicked() {
                                self.call = Some((function.trap, function.number));
                            }
                        }
                    });
                }
                egui::CollapsingHeader::new("Line-A ($A000-$A00F)").id_salt("st-calls-linea").show(ui, |ui| {
                    for (n, (name, about)) in st_tos::LINE_A.iter().enumerate() {
                        ui.label(RichText::new(format!("$A00{n:X}  {name}")).monospace()).on_hover_text(*about);
                    }
                });
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(480.0);
                let Some(function) = self.call.and_then(|(trap, n)| st_tos::function(trap, n as i64)) else {
                    ui.label("Choose a function.");
                    return;
                };
                ui.label(RichText::new(function.describe()).monospace());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Put a call in at the cursor").on_hover_text("Its arguments pushed, last first, with 0 for each to fill in").clicked() {
                        insert = Some(call_source(function));
                    }
                    if ui.button("Copy as an EQU").clicked() {
                        ui.ctx().copy_text(function.equate());
                    }
                });
            });
        });
        insert
    }
}

impl Limoniter {
    pub(super) fn open_atari_hardware(&mut self) {
        self.st_tools.hardware.get_or_insert_with(Box::default);
    }

    pub(super) fn open_atari_blitter(&mut self) {
        self.st_tools.blitter.get_or_insert_with(Box::default);
    }

    pub(super) fn open_atari_timers(&mut self) {
        self.st_tools.timers.get_or_insert_with(Box::default);
    }

    /// Text from one of the windows, at the cursor in the file in front, as one undoable change.
    fn insert_st_text(&mut self, title: &str, text: &str) {
        if self.no_file() || self.docs[self.active].hex_mode || self.docs[self.active].text.read_only {
            self.message(title, "Open the source to put it in, with the cursor where it goes.".into());
            return;
        }
        let doc = &mut self.docs[self.active];
        doc.text.begin_undo_group(false);
        doc.text.type_text(text);
        doc.text.end_undo_group();
    }

    pub(super) fn st_tools_windows(&mut self, ctx: &Context) {
        if let Some(mut window) = self.st_tools.hardware.take() {
            let mut open = true;
            let mut insert = None;
            egui::Window::new("Atari ST hardware").open(&mut open).default_pos([150.0, 100.0]).default_width(770.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut window.tab, Tab::Registers, "Registers");
                    ui.selectable_value(&mut window.tab, Tab::Calls, "TOS calls");
                });
                ui.separator();
                insert = match window.tab {
                    Tab::Registers => window.registers_tab(ui),
                    Tab::Calls => window.calls_tab(ui),
                };
            });
            if let Some(text) = insert {
                self.insert_st_text("Atari ST hardware", &text);
            }
            if open {
                self.st_tools.hardware = Some(window);
            }
        }
        if let Some(mut blitter) = self.st_tools.blitter.take() {
            let mut open = true;
            let mut asked = None;
            egui::Window::new("STE blitter").open(&mut open).resizable(false).default_pos([230.0, 100.0]).show(ctx, |ui| {
                asked = blitter.draw(ui);
            });
            if let Some(crate::st_blitter::Asked::Insert(text)) = asked {
                self.insert_st_text("STE blitter", &text);
            }
            if open {
                self.st_tools.blitter = Some(blitter);
            }
        }
        if let Some(mut timers) = self.st_tools.timers.take() {
            let mut open = true;
            let mut asked = None;
            egui::Window::new("MFP timers").open(&mut open).resizable(false).default_pos([270.0, 120.0]).show(ctx, |ui| {
                asked = timers.draw(ui);
            });
            if let Some(crate::mfp_timer::Asked::Insert(text)) = asked {
                self.insert_st_text("MFP timers", &text);
            }
            if open {
                self.st_tools.timers = Some(timers);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_and_calls_are_found_by_name_address_or_words() {
        let tbcr = st_hw::at(0xFFFA1B).unwrap();
        assert!(matches(tbcr, "tbcr"));
        assert!(matches(tbcr, "$fffa1b"));
        assert!(matches(tbcr, "fffffa1b"));
        assert!(matches(tbcr, "$fa1b.w"));
        assert!(matches(tbcr, "raster"));
        assert!(!matches(tbcr, "palette"));
        let cconws = st_tos::named("Cconws").unwrap();
        assert!(call_matches(cconws, "cconws") && call_matches(cconws, "9") && call_matches(cconws, "string") && !call_matches(cconws, "floppy"));
        let text = call_source(st_tos::named("Fread").unwrap());
        assert!(text.starts_with("        move.l  #0,-(sp)                ; buffer") && text.contains("move.w  #$3f,-(sp)") && text.contains("lea     12(sp),sp"), "{text}");
        let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::M68k, std::path::Path::new("t.s"), &text);
        assert!(assembly.assembly.ok());
    }

    #[test]
    fn the_windows_draw_and_put_text_in() {
        let (mut app, ctx) = super::super::tests::app_with("");
        app.open_atari_hardware();
        app.open_atari_blitter();
        app.open_atari_timers();
        for tab in [Tab::Registers, Tab::Calls] {
            app.st_tools.hardware.as_mut().unwrap().tab = tab;
            app.st_tools.hardware.as_mut().unwrap().value = "$777".into();
            for _ in 0..2 {
                ctx.run_ui(Default::default(), |ui| app.st_tools_windows(ui.ctx())).textures_delta.clear();
            }
        }
        assert!(app.st_tools.hardware.is_some() && app.st_tools.blitter.is_some() && app.st_tools.timers.is_some());
        app.insert_st_text("MFP timers", &crate::mfp_timer::source(crate::mfp_timer::Timer::B, 8, 8, "raster", "every 8 lines"));
        assert_eq!(&*app.doc().text.line(0), "        move.l  #raster,$120.w          ; Timer B's vector");
    }

    #[test]
    fn the_atari_starters_are_on_new_as() {
        let (mut app, _ctx) = super::super::tests::app_with("");
        for (name, text) in [("Atari ST demo (68000)", crate::st_starters::ST_DEMO), ("Atari STE demo (68000)", crate::st_starters::STE_DEMO)] {
            let index = app.new_kinds.iter().position(|k| k.name == name).unwrap();
            app.new_of_kind(index);
            assert_eq!(app.docs[app.active].text.all_lines().len(), text.lines().count());
            assert_eq!(app.doc_cpu(app.active), Some(crate::assemblers::Cpu::M68k));
        }
    }
}
