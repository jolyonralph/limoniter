//! The Commodore 64 hardware window: the VIC-II's, SID's, CIAs' and the processor port's registers and the
//! KERNAL's routines and vectors, searchable and grouped by chip, with any value read into its bits (a colour
//! shown in the Pepto and Colodore palettes, a note for a frequency, the ROMs a $01 value switches in); and a
//! calculator for where the VIC-II finds the screen, characters, bitmap and sprites for a $D018 and $DD00. It's
//! kept open beside the text, putting what it makes at the cursor.

use super::*;
use crate::c64_hw::{self, Access, Chip, Register};
use egui::{Color32, RichText, Sense, vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Registers,
    Memory,
}

/// The window's state while it's open.
pub(super) struct HwWindow {
    pub tab: Tab,
    search: String,
    selected: u16,
    pub value: String,
    /// For the SID's frequencies: whether they're worked out for a PAL machine.
    pal: bool,
    /// The memory calculator: the bank (0-3), the screen's slot (0-15), the characters' (0-7), the bitmap's (0-1),
    /// and a sprite pointer.
    bank: u8,
    screen: u8,
    charset: u8,
    bitmap: u8,
    pointer: u8,
    d018_text: String,
    dd00_text: String,
    /// A note to find the SID value of, in Hz.
    hz: String,
}

impl Default for HwWindow {
    fn default() -> Self {
        HwWindow { tab: Tab::Registers, search: String::new(), selected: 0xD020, value: String::new(), pal: true, bank: 0, screen: 1, charset: 2, bitmap: 0, pointer: 13, d018_text: String::new(), dd00_text: String::new(), hz: "440".into() }
    }
}

/// The Commodore 64 windows, while they're open.
#[derive(Default)]
pub(super) struct C64Tools {
    pub hardware: Option<Box<HwWindow>>,
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
    if let Ok(n) = u16::from_str_radix(search.trim_start_matches('$').trim_start_matches("0x"), 16)
        && search.len() >= 3
        && c64_hw::at(n).is_some_and(|r| r.address == register.address)
    {
        return true;
    }
    register.name.to_ascii_lowercase().contains(&search)
        || register.title.to_ascii_lowercase().contains(&search)
        || register.about.to_ascii_lowercase().contains(&search)
        || register.fields.iter().any(|f| f.name.to_ascii_lowercase().contains(&search) || f.about.to_ascii_lowercase().contains(&search))
}

fn swatch(ui: &mut egui::Ui, colour: u32, tip: &str) -> egui::Response {
    let [r, g, b] = c64_hw::rgb(colour);
    let (rect, response) = ui.allocate_exact_size(vec2(22.0, 14.0), Sense::click());
    ui.painter().rect_filled(rect, 2.0, Color32::from_rgb(r, g, b));
    ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, ui.visuals().weak_text_color()), egui::StrokeKind::Inside);
    response.on_hover_text(tip)
}

/// The lines setting the VIC up as the calculator says: the bank in $DD00 (keeping its other bits) and $D018.
pub(super) fn memory_source(dd00: u8, d018: u8, words: &str) -> String {
    format!(
        "        lda $dd00\n        and #%11111100\n        ora #%{:02b}           ; VIC bank ${:04X}\n        sta $dd00\n        lda #${d018:02x}\n        sta $d018           ; {words}\n",
        dd00 & 3,
        c64_hw::bank_of(dd00)
    )
}

impl HwWindow {
    fn registers_tab(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let mut insert = None;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search: name, address or words").desired_width(240.0));
            ui.checkbox(&mut self.pal, "PAL").on_hover_text("Work out the SID's notes and the CIA timers for a PAL machine (985248 Hz) rather than NTSC (1022727 Hz)");
        });
        ui.separator();
        let searching = !self.search.trim().is_empty();
        ui.horizontal_top(|ui| {
            egui::ScrollArea::vertical().id_salt("c64-hw-list").max_height(440.0).min_scrolled_height(440.0).show(ui, |ui| {
                ui.set_width(260.0);
                for chip in Chip::ALL {
                    let shown: Vec<&Register> = c64_hw::registers().iter().filter(|r| r.chip == chip && matches(r, &self.search)).collect();
                    if shown.is_empty() {
                        continue;
                    }
                    let header = egui::CollapsingHeader::new(format!("{} ({})", chip.name(), shown.len())).id_salt(("c64-hw-chip", chip as u8));
                    let header = if searching { header.open(Some(true)) } else { header.default_open(chip == Chip::Vic) };
                    header.show(ui, |ui| {
                        for register in shown {
                            let label = format!("${:04X}  {:<7} {}", register.address, register.name, register.title);
                            if ui.selectable_label(register.address == self.selected, RichText::new(label).monospace()).on_hover_text(&register.about).clicked() {
                                self.selected = register.address;
                            }
                        }
                    });
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(480.0);
                let Some(register) = c64_hw::at(self.selected) else {
                    ui.label("Choose a register.");
                    return;
                };
                ui.label(RichText::new(format!("{}: {}", register.name, register.title)).strong().size(16.0));
                ui.label(RichText::new(register.heading()).monospace().small());
                ui.add_space(4.0);
                ui.add(egui::Label::new(&register.about).wrap());
                ui.add_space(6.0);
                if !register.fields.is_empty() {
                    egui::Grid::new("c64-hw-bits").striped(true).num_columns(3).show(ui, |ui| {
                        for f in register.fields {
                            ui.label(RichText::new(f.bits()).monospace());
                            ui.label(RichText::new(f.name).monospace().strong());
                            ui.add(egui::Label::new(f.about).wrap());
                            ui.end_row();
                        }
                    });
                    ui.add_space(6.0);
                }
                if register.is_colour() {
                    ui.label("The colours (Pepto's, then Colodore's): click one to use it.");
                    for (palette, colours) in [("Pepto", &c64_hw::PEPTO), ("Colodore", &c64_hw::COLODORE)] {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{palette:<9}")).monospace().small());
                            for (n, colour) in colours.iter().enumerate() {
                                if swatch(ui, *colour, &format!("{n}: {} (#{colour:06X})", c64_hw::COLOUR_NAMES[n])).clicked() {
                                    self.value = n.to_string();
                                }
                            }
                        });
                    }
                    ui.add_space(4.0);
                }
                if register.access != Access::Call && register.access != Access::Vector {
                    ui.horizontal(|ui| {
                        ui.label("Value:");
                        ui.add(egui::TextEdit::singleline(&mut self.value).hint_text("$1b").desired_width(100.0).font(egui::TextStyle::Monospace));
                        if ui.button("Copy name").clicked() {
                            ui.ctx().copy_text(register.name.clone());
                        }
                        if ui.button("Copy the chip's names").on_hover_text(format!("Every register of {} as NAME = $D020 lines", register.chip.name())).clicked() {
                            ui.ctx().copy_text(c64_hw::equates(Some(register.chip)));
                        }
                        if ui.button("Put them in").on_hover_text("The NAME = $D020 lines at the cursor").clicked() {
                            insert = Some(c64_hw::equates(Some(register.chip)));
                        }
                    });
                    if let Some(value) = number(&self.value) {
                        let value = value as u8;
                        let said = c64_hw::describe_value(register, value);
                        for (i, line) in said.lines().skip(1).enumerate() {
                            let text = RichText::new(line.replace('\u{25A0}', ""));
                            ui.label(if i == 0 { text.strong() } else { text.monospace().small() });
                        }
                        if register.is_colour() {
                            ui.horizontal(|ui| {
                                swatch(ui, c64_hw::PEPTO[(value & 15) as usize], "Pepto");
                                swatch(ui, c64_hw::COLODORE[(value & 15) as usize], "Colodore");
                                ui.label(c64_hw::COLOUR_NAMES[(value & 15) as usize]);
                            });
                        }
                        if let Some(voice) = register.voice().filter(|_| (register.address - 0xD400) % 7 <= 1) {
                            let hz = c64_hw::sid_hz(if (register.address - 0xD400) % 7 == 1 { (value as u16) << 8 } else { value as u16 }, self.pal);
                            ui.label(RichText::new(format!("Voice {voice}: this byte alone is {hz:.2} Hz on {}", if self.pal { "PAL" } else { "NTSC" })).small());
                        }
                        if register.address == 0x0001 {
                            let [basic, io, kernal] = c64_hw::memory_setup(value);
                            egui::Grid::new("c64-hw-port").num_columns(2).show(ui, |ui| {
                                for (range, what) in [("$0000-$9FFF", "RAM"), ("$A000-$BFFF", basic), ("$C000-$CFFF", "RAM"), ("$D000-$DFFF", io), ("$E000-$FFFF", kernal)] {
                                    ui.label(RichText::new(range).monospace());
                                    ui.label(what);
                                    ui.end_row();
                                }
                            });
                        }
                    } else if !self.value.trim().is_empty() {
                        ui.label("That isn't a number: write $1b, %00011011 or 27.");
                    }
                    if register.voice().is_some() && (register.address - 0xD400) % 7 <= 1 {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("A note's value: Hz");
                            ui.add(egui::TextEdit::singleline(&mut self.hz).desired_width(70.0));
                            if let Ok(hz) = self.hz.trim().parse::<f64>() {
                                let v = c64_hw::sid_value(hz, self.pal);
                                let note = c64_hw::note(hz).map(|(n, c)| format!(" ({n} {c:+.0} cents)")).unwrap_or_default();
                                ui.label(RichText::new(format!("${v:04X}: low ${:02X}, high ${:02X}{note}", v & 0xFF, v >> 8)).monospace());
                            }
                        });
                    }
                } else if ui.button("Copy name").clicked() {
                    ui.ctx().copy_text(register.name.clone());
                }
            });
        });
        insert
    }

    fn memory_tab(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let mut insert = None;
        ui.label("Where the VIC-II finds the screen, the characters, a bitmap and the sprites: the 16K bank CIA 2 chooses in $DD00, and the places in it $D018 chooses.");
        ui.add_space(6.0);
        // Typed values set the choices.
        ui.horizontal(|ui| {
            ui.label("$D018:");
            if ui.add(egui::TextEdit::singleline(&mut self.d018_text).hint_text("$15").desired_width(60.0).font(egui::TextStyle::Monospace)).changed()
                && let Some(v) = number(&self.d018_text)
            {
                let v = v as u8;
                self.screen = v >> 4;
                self.charset = (v >> 1) & 7;
                self.bitmap = (v >> 3) & 1;
            }
            ui.label("$DD00:");
            if ui.add(egui::TextEdit::singleline(&mut self.dd00_text).hint_text("$97").desired_width(60.0).font(egui::TextStyle::Monospace)).changed()
                && let Some(v) = number(&self.dd00_text)
            {
                self.bank = 3 - (v as u8 & 3);
            }
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("c64-bank").selected_text(format!("Bank {} (${:04X})", self.bank, self.bank as u16 * 0x4000)).show_ui(ui, |ui| {
                for b in 0..4u8 {
                    ui.selectable_value(&mut self.bank, b, format!("Bank {b}: ${:04X}-${:04X}", b as u16 * 0x4000, b as u16 * 0x4000 + 0x3FFF));
                }
            });
            let base = self.bank as u16 * 0x4000;
            egui::ComboBox::from_id_salt("c64-screen").selected_text(format!("Screen ${:04X}", base + self.screen as u16 * 0x400)).show_ui(ui, |ui| {
                for s in 0..16u8 {
                    ui.selectable_value(&mut self.screen, s, format!("${:04X}", base + s as u16 * 0x400));
                }
            });
            egui::ComboBox::from_id_salt("c64-charset").selected_text(format!("Characters ${:04X}", base + self.charset as u16 * 0x800)).show_ui(ui, |ui| {
                for c in 0..8u8 {
                    let at = base + c as u16 * 0x800;
                    let rom = c64_hw::sees_rom(at).map(|r| format!(" ({r})")).unwrap_or_default();
                    ui.selectable_value(&mut self.charset, c, format!("${at:04X}{rom}"));
                }
            });
        });
        let d018 = (self.screen << 4) | (self.charset << 1);
        let dd00 = 0x94 | (3 - self.bank);
        let memory = c64_hw::vic_memory(d018, dd00);
        ui.add_space(6.0);
        egui::Grid::new("c64-memory").striped(true).num_columns(2).show(ui, |ui| {
            let rows = [
                ("$D018", format!("${d018:02X} (%{d018:08b})")),
                ("$DD00 bits 0-1", format!("%{:02b}  (ORA into $DD00)", dd00 & 3)),
                ("Screen", format!("${:04X}-${:04X}{}", memory.screen, memory.screen + 999, c64_hw::sees_rom(memory.screen).map(|r| format!(": the VIC sees {r} there, not RAM")).unwrap_or_default())),
                ("Characters", format!("${:04X}-${:04X}{}", memory.charset, memory.charset + 0x7FF, c64_hw::sees_rom(memory.charset).map(|r| format!(" ({r})")).unwrap_or_default())),
                ("Bitmap (bit 3 of $D018)", format!("${:04X}-${:04X}", memory.bitmap, memory.bitmap + 0x1F3F)),
                ("Sprite pointers", format!("${:04X}-${:04X}", memory.sprite_pointers, memory.sprite_pointers + 7)),
            ];
            for (what, said) in rows {
                ui.label(what);
                ui.label(RichText::new(said).monospace());
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            ui.label("A sprite pointer:");
            ui.add(egui::DragValue::new(&mut self.pointer).range(0..=255));
            let at = memory.sprite(self.pointer);
            let rom = c64_hw::sees_rom(at).map(|r| format!(", in {r}")).unwrap_or_default();
            ui.label(RichText::new(format!("the sprite's 63 bytes at ${at:04X}-${:04X}{rom}", at + 62)).monospace());
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Put the setup in at the cursor").on_hover_text("LDA/STA lines setting $DD00's bank and $D018").clicked() {
                insert = Some(memory_source(dd00, d018, &memory.words(false)));
            }
            if ui.button("Copy it").clicked() {
                ui.ctx().copy_text(memory_source(dd00, d018, &memory.words(false)));
            }
        });
        insert
    }
}

impl Limoniter {
    pub(super) fn open_c64_hardware(&mut self) {
        self.c64_tools.hardware.get_or_insert_with(Box::default);
    }

    /// Text from the window, at the cursor in the file in front, as one undoable change.
    fn insert_c64_text(&mut self, title: &str, text: &str) {
        if self.no_file() || self.docs[self.active].hex_mode || self.docs[self.active].text.read_only {
            self.message(title, "Open the source to put it in, with the cursor where it goes.".into());
            return;
        }
        let doc = &mut self.docs[self.active];
        doc.text.begin_undo_group(false);
        doc.text.type_text(text);
        doc.text.end_undo_group();
    }

    pub(super) fn c64_tools_windows(&mut self, ctx: &Context) {
        if let Some(mut window) = self.c64_tools.hardware.take() {
            let mut open = true;
            let mut insert = None;
            egui::Window::new("Commodore 64 hardware").open(&mut open).default_pos([160.0, 110.0]).default_width(770.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut window.tab, Tab::Registers, "Registers and KERNAL");
                    ui.selectable_value(&mut window.tab, Tab::Memory, "VIC memory");
                });
                ui.separator();
                insert = match window.tab {
                    Tab::Registers => window.registers_tab(ui),
                    Tab::Memory => window.memory_tab(ui),
                };
            });
            if let Some(text) = insert {
                self.insert_c64_text("Commodore 64 hardware", &text);
            }
            if open {
                self.c64_tools.hardware = Some(window);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_are_found_by_name_address_or_words() {
        let d011 = c64_hw::at(0xD011).unwrap();
        assert!(matches(d011, "scroly") && matches(d011, "$d011") && matches(d011, "d011") && matches(d011, "bitmap"));
        assert!(!matches(d011, "volume"));
        assert!(matches(c64_hw::named("CHROUT").unwrap(), "ffd2"));
        let text = memory_source(0x96, 0x18, "screen $4400");
        assert!(text.contains("ora #%10") && text.contains("lda #$18"), "{text}");
        let assembled = crate::assemblers::assemble_own(crate::assemblers::Cpu::M6502, std::path::Path::new("t.asm"), &format!("  *= $c000\n{text}"));
        assert!(assembled.assembly.ok(), "{:?}", assembled.assembly.problems);
    }

    /// With LIMONITER_VICE naming VICE's x64sc (LIMONITER_VICE_OPTIONS its ROMs) and LIMONITER_KICKASS and
    /// LIMONITER_64TASS the assemblers: source each assembles stops at its breakpoint in VICE, on its line, and
    /// steps on to the next.
    #[test]
    fn kickassembler_and_64tass_source_is_debugged_in_a_real_vice() {
        use crate::assemblers::{Choice, Cpu};
        use crate::deploy::{Device, Place};
        use std::time::Duration;
        let Some(vice) = std::env::var_os("LIMONITER_VICE") else {
            return;
        };
        let options = std::env::var("LIMONITER_VICE_OPTIONS").unwrap_or_default();
        crate::highlight::load_now();
        let folder = std::env::temp_dir().join(format!("limoniter-c64-debug-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        for (variable, id, text) in [
            ("LIMONITER_KICKASS", "kickass", "* = $c000 \"Code\"\nstart:  lda #7\n        ldx #$12\n        jsr sub\n        ldy #$34\nloop:   jmp loop\nsub:    inx\n        rts\n"),
            ("LIMONITER_64TASS", "64tass", "        *= $c000\nstart   lda #7\n        ldx #$12\n        jsr sub\n        ldy #$34\nloop    jmp loop\nsub     inx\n        rts\n"),
        ] {
            let Some(assembler) = std::env::var_os(variable) else { continue };
            let (mut app, ctx) = super::super::tests::app_with("");
            app.sends.devices = vec![Device { name: "VICE".into(), place: Place::Emulator { emulator: "vice".into(), program: vice.clone().into(), options: options.clone() }, preset: None, eject: false }];
            app.set_assembler(Cpu::M6502, &Choice::Program { id: crate::assemblers::external(id).unwrap().id, path: assembler.into() });
            let path = folder.join(format!("{id}.asm"));
            std::fs::write(&path, text).unwrap();
            assert!(app.open_path(&path));
            app.doc().update_syntax();
            app.assembly_tick();
            app.doc().text.code.as_mut().unwrap().breakpoints.insert(2);
            app.run_on_target(run_host::Target::Device(0));
            assert!(matches!(app.dialog, Dialog::None), "{id}: {:?}", app.script_output);
            let stopped_at = |app: &mut Limoniter| {
                let began = std::time::Instant::now();
                while began.elapsed() < Duration::from_secs(40) {
                    ctx.run_ui(Default::default(), |ui| app.remote_debug_panel(ui)).textures_delta.clear();
                    if let Some(line) = app.doc().text.code.as_ref().and_then(|c| c.stopped) {
                        return Some(line);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                None
            };
            assert_eq!(stopped_at(&mut app), Some(2), "{id}: stopped at its breakpoint, on line 3: {:?}", app.script_output);
            match &mut app.running.sessions[0].kind {
                run_host::SessionKind::App { debug: Some(d), .. } => d.remote.ask(crate::remote_debug::Ask::Step),
                _ => panic!("it's being debugged"),
            }
            ctx.run_ui(Default::default(), |ui| app.remote_debug_panel(ui)).textures_delta.clear();
            assert_eq!(stopped_at(&mut app), Some(3), "{id}: a step on, at the JSR");
            app.stop_all_sessions();
        }
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_window_draws_and_puts_text_in() {
        let (mut app, ctx) = super::super::tests::app_with("");
        app.open_c64_hardware();
        for (tab, selected) in [(Tab::Registers, 0xD020), (Tab::Registers, 0xD401), (Tab::Registers, 0x0001), (Tab::Registers, 0xFFD2), (Tab::Memory, 0xD018)] {
            let window = app.c64_tools.hardware.as_mut().unwrap();
            window.tab = tab;
            window.selected = selected;
            window.value = "$35".into();
            for _ in 0..2 {
                ctx.run_ui(Default::default(), |ui| app.c64_tools_windows(ui.ctx())).textures_delta.clear();
            }
        }
        assert!(app.c64_tools.hardware.is_some());
        app.insert_c64_text("Commodore 64 hardware", &memory_source(0x97, 0x15, "screen $0400"));
        assert_eq!(&*app.doc().text.line(0), "        lda $dd00");
    }
}
