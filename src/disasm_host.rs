//! The window's side of disassembling: Tools, Disassemble, on the file in front. What the file is says what to
//! take, where it loads and where it starts (a C64 program's load address and SYS, a snapshot's PC, a tape's CODE
//! block, a code file from a tape), and a box lets those be changed before the source opens in a new tab.

use super::*;
use crate::assemblers::Cpu;

/// The box asking how to disassemble.
pub(super) struct DisasmBox {
    pub cpu: Cpu,
    pub bytes: Vec<u8>,
    pub origin: String,
    pub starts: String,
    pub stem: String,
    /// What was found out about the file, to say.
    pub found: String,
    pub error: String,
}

/// What a file holds to disassemble: the processor, the code, where it loads, where it starts, and how that was
/// known.
pub(super) fn guess(name: &str, bytes: &[u8], load_address: Option<u16>) -> (Cpu, Vec<u8>, u16, Vec<u16>, String) {
    let ending = Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    // A Commodore program: its load address, and the SYS its BASIC line calls, if it has one.
    if matches!(ending.as_str(), "prg" | "p00") && bytes.len() > 2 {
        let skip = if ending == "p00" { 26 } else { 0 };
        let origin = u16::from_le_bytes([bytes[skip], bytes[skip + 1]]);
        let code = bytes[skip + 2..].to_vec();
        let mut starts = vec![origin];
        if let Some(at) = code.iter().position(|b| *b == 0x9E) {
            let digits: String = code[at + 1..].iter().map(|b| char::from(*b)).skip_while(|c| *c == ' ').take_while(char::is_ascii_digit).collect();
            if let Ok(sys) = digits.parse::<u16>() {
                starts = vec![sys];
            }
        }
        return (Cpu::M6502, code, origin, starts, format!("A Commodore program, loading at ${origin:04X}"));
    }
    // A 48K snapshot: the 48K from $4000, starting where it was saved.
    let sna = match ending.as_str() {
        "sna" if bytes.len() == 49179 => Some(bytes.to_vec()),
        "z80" => crate::zx_machine::z80_to_sna(bytes).ok().filter(|s| s.len() == 49179),
        _ => None,
    };
    if let Some(sna) = sna {
        let memory = sna[27..].to_vec();
        let sp = u16::from_le_bytes([sna[23], sna[24]]);
        let pc = memory.get(usize::from(sp.wrapping_sub(0x4000))..).and_then(|m| m.get(..2)).map_or(0x4000, |b| u16::from_le_bytes([b[0], b[1]]));
        return (Cpu::Z80, memory, 0x4000, vec![pc], format!("A 48K snapshot, its 48K from $4000, saved at PC ${pc:04X}"));
    }
    // A tape: its first CODE block.
    if matches!(ending.as_str(), "tap" | "tzx") {
        let (blocks, _) = basic::tape_blocks(bytes);
        let mut header: Option<u16> = None;
        for block in blocks {
            match (block.first(), header) {
                (Some(0), _) if block.len() >= 19 && block[1] == 3 => header = Some(u16::from_le_bytes([block[14], block[15]])),
                (Some(0xFF), Some(start)) => {
                    let code = block[1..block.len() - 1].to_vec();
                    return (Cpu::Z80, code, start, vec![start], format!("The tape's CODE block, {} bytes at ${start:04X}", block.len() - 2));
                }
                _ => header = None,
            }
        }
    }
    // A Color Computer binary: its first block, and where it says it starts.
    if let Some(crate::running::Machine::CoCo) = crate::running::machine_for(crate::running::Format::CocoBin, bytes)
        && bytes.len() > 10
    {
        let length = usize::from(u16::from_be_bytes([bytes[1], bytes[2]]));
        let origin = u16::from_be_bytes([bytes[3], bytes[4]]);
        let exec = u16::from_be_bytes([bytes[bytes.len() - 2], bytes[bytes.len() - 1]]);
        let code = bytes.get(5..5 + length).unwrap_or(&[]).to_vec();
        return (Cpu::M6809, code, origin, vec![exec], format!("A Color Computer binary, {length} bytes at ${origin:04X}, starting at ${exec:04X}"));
    }
    let origin = load_address.unwrap_or(0x8000);
    let found = match load_address {
        Some(at) => format!("A code file from a tape, loading at ${at:04X}"),
        None => format!("{} bytes; say where they load", bytes.len()),
    };
    (Cpu::Z80, bytes.to_vec(), origin, vec![origin], found)
}

/// A number as typed: $8000, 0x8000, #8000, 8000h or 32768.
fn number(text: &str) -> Option<u16> {
    let t = text.trim();
    let (digits, radix) = if let Some(h) = t.strip_prefix('$').or_else(|| t.strip_prefix('#')).or_else(|| t.strip_prefix("0x")).or_else(|| t.strip_prefix("0X")) {
        (h, 16)
    } else if let Some(h) = t.strip_suffix('h').or_else(|| t.strip_suffix('H')) {
        (h, 16)
    } else {
        (t, 10)
    };
    u32::from_str_radix(digits, radix).ok().and_then(|n| u16::try_from(n).ok())
}

impl Limoniter {
    pub(super) fn open_disassemble_box(&mut self) {
        if self.no_file() {
            return;
        }
        let doc = &self.docs[self.active];
        let name = doc.path_or_name().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let stem = Path::new(&name).file_stem().map_or_else(|| "program".to_string(), |s| s.to_string_lossy().into_owned());
        let load_address = doc.load_address;
        let bytes = self.document_bytes();
        if bytes.is_empty() {
            return self.message("Disassemble", "There's nothing in this file to disassemble.".into());
        }
        let (cpu, code, origin, starts, found) = guess(&name, &bytes, load_address);
        let starts = starts.iter().map(|s| format!("${s:04X}")).collect::<Vec<_>>().join(", ");
        self.dialog = Dialog::Disassemble(Box::new(DisasmBox { cpu, bytes: code, origin: format!("${origin:04X}"), starts, stem, found, error: String::new() }));
    }

    pub(super) fn disassemble_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::Disassemble(state) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut go = false;
        egui::Window::new("Disassemble").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(RichText::new(&state.found).color(theme::text_dim()));
            ui.add_space(6.0);
            egui::Grid::new("disasm-grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Processor");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut state.cpu, Cpu::Z80, "Z80 (and the Next's)");
                    ui.radio_value(&mut state.cpu, Cpu::M6502, "6502");
                    ui.radio_value(&mut state.cpu, Cpu::M6809, "6809");
                });
                ui.end_row();
                ui.label("Loads at");
                ui.add(egui::TextEdit::singleline(&mut state.origin).desired_width(120.0));
                ui.end_row();
                ui.label("Starts at");
                ui.add(egui::TextEdit::singleline(&mut state.starts).hint_text("$8000, $8100...").desired_width(200.0));
                ui.end_row();
            });
            ui.label(RichText::new("The code is followed from where it starts, through every jump and call; what it never reaches is kept as data. Add more places it starts, comma between, for code reached some other way (interrupts, tables of addresses).").color(theme::text_faint()).size(11.5));
            if !state.error.is_empty() {
                ui.colored_label(theme::change_unsaved(), &state.error);
            }
            ui.horizontal(|ui| {
                go = ui.button("Disassemble").clicked();
                close |= ui.button("Cancel").clicked();
            });
        });
        if go {
            let Dialog::Disassemble(state) = &mut self.dialog else {
                return true;
            };
            let Some(origin) = number(&state.origin) else {
                state.error = "Say where it loads: $8000, or 32768.".into();
                return true;
            };
            let starts: Option<Vec<u16>> = state.starts.split(',').filter(|s| !s.trim().is_empty()).map(number).collect();
            let Some(starts) = starts else {
                state.error = "Say where it starts: addresses, comma between.".into();
                return true;
            };
            if usize::from(origin) + state.bytes.len() > 0x10000 {
                state.error = format!("{} bytes from ${origin:04X} go past the end of 64K.", state.bytes.len());
                return true;
            }
            let (cpu, stem) = (state.cpu, state.stem.clone());
            let source = crate::disasm::disassemble_checked(cpu, &state.bytes, origin, &starts);
            self.dialog = Dialog::None;
            let mut doc = Document::untitled(format!("{stem}.asm"), &PRESETS[0]);
            doc.text.set_content(source.lines().map(str::to_string).collect());
            highlight::load_now();
            let language = match cpu {
                Cpu::M6502 => "Assembly 6502",
                Cpu::M6809 => "Assembly 6809",
                _ => "Assembly Z80",
            };
            if let Some((_, syntax)) = highlight::engine().and_then(|e| e.names().iter().find(|(n, _)| n == language)) {
                doc.set_language(Language::Named(*syntax));
            }
            doc.update_syntax();
            self.docs.push(doc);
            self.active = self.docs.len() - 1;
            self.pending_focus = true;
        } else if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_file_holds_is_worked_out() {
        // A C64 program with 10 SYS 2064.
        let mut prg = vec![0x01, 0x08, 0x0C, 0x08, 0x0A, 0x00, 0x9E, b'2', b'0', b'6', b'4', 0, 0, 0];
        prg.extend([0xA9, 0x02, 0x8D, 0x20, 0xD0, 0x60]);
        let (cpu, code, origin, starts, _) = guess("border.prg", &prg, None);
        assert_eq!((cpu, origin, starts), (Cpu::M6502, 0x0801, vec![2064]));
        assert_eq!(code.len(), prg.len() - 2);
        // A tape: its CODE block.
        let assembly = crate::asm::assemble(&crate::asm::z80::Z80, Path::new("t.asm"), "  org 40000\nstart: ld a,2\n  out (254),a\n  ret\n", &|_| Err("no".into()));
        let tape = crate::asm::output::spectrum_tape(&assembly, "t").unwrap();
        let (cpu, code, origin, starts, _) = guess("t.tap", &tape, None);
        assert_eq!((cpu, origin, starts, code), (Cpu::Z80, 40000, vec![40000], vec![0x3E, 2, 0xD3, 0xFE, 0xC9]));
        // Anything else: where a tape said it loads, or $8000.
        assert_eq!(guess("udgs.code", &[1, 2], Some(65368)).2, 65368);
        assert_eq!(number("$8000"), Some(0x8000));
        assert_eq!(number("8000h"), Some(0x8000));
        assert_eq!(number("32768"), Some(32768));
        assert_eq!(number("70000"), None);
    }

    #[test]
    fn a_tape_is_disassembled_into_a_new_tab() {
        let (mut app, _ctx) = super::super::tests::app_with("");
        let assembly = crate::asm::assemble(&crate::asm::z80::Z80, Path::new("t.asm"), "  org 40000\nstart: ld b,4\nloop: djnz loop\n  jp start\n", &|_| Err("no".into()));
        let tape = crate::asm::output::spectrum_tape(&assembly, "t").unwrap();
        app.docs.push(Document::from_image_file("game.tap".into(), tape, None));
        app.active = app.docs.len() - 1;
        app.open_disassemble_box();
        let Dialog::Disassemble(state) = &app.dialog else { panic!("the box opens") };
        assert_eq!((state.origin.as_str(), state.starts.as_str()), ("$9C40", "$9C40"));
        let (cpu, bytes) = (state.cpu, state.bytes.clone());
        let source = crate::disasm::disassemble_checked(cpu, &bytes, 40000, &[40000]);
        assert!(source.contains("L9C42:  djnz L9C42") && source.contains("jp L9C40"), "{source}");
    }
}
