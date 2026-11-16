//! The window's side of disassembling: Tools, Disassemble, on the file in front. What the file is says what to
//! take, where it loads and where it starts (a C64 program's load address and SYS, a snapshot's PC, a tape's CODE
//! block, a code file from a tape), and a box lets those be changed before the source opens in a new tab.

use super::*;
use crate::assemblers::Cpu;

/// The box asking how to disassemble.
pub(super) struct DisasmBox {
    pub cpu: Cpu,
    /// ICL 2900 code, listed rather than traced, in place of `cpu`.
    pub icl2900: bool,
    /// x86 code, in place of `cpu`: the program the file was found to be, or plain code in `x86_bits`' mode.
    pub x86: bool,
    pub x86_program: Option<crate::x86::Program>,
    pub x86_bits: u32,
    /// Z80 code for a Cambridge Z88, whose RST &20 calls are named.
    pub z88: bool,
    /// 68000 code: the program the file was found to be (an Amiga or Atari ST program, a boot block, a cartridge
    /// or a ROM), or plain code for `machine`.
    pub m68k_program: Option<crate::m68k_disasm::Program>,
    pub machine: crate::m68k_disasm::Machine,
    /// An Atari 2600 cartridge, disassembled bank by bank into DASM source.
    pub vcs: Option<crate::atari2600::Cartridge>,
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
    // A SID tune: its C64 bytes at its load address, from its init and play routines.
    if ending == "sid"
        && let Ok(tune) = crate::sid::read(bytes)
    {
        let starts: Vec<u16> = [tune.init_address(), tune.play].into_iter().filter(|a| *a != 0).collect();
        let found = format!("A SID tune, loading at ${:04X}, init ${:04X}{}", tune.load, tune.init_address(), if tune.play == 0 { String::new() } else { format!(", play ${:04X}", tune.play) });
        return (Cpu::M6502, tune.data, tune.load, if starts.is_empty() { vec![tune.load] } else { starts }, found);
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
    // An Atari binary load file: its segments laid out in memory, followed from its init and run addresses; or one
    // segment of one, as its browser names it.
    if matches!(ending.as_str(), "xex" | "obx" | "com" | "exe" | "sys" | "obj")
        && let Some((memory, origin, starts, how)) = crate::media::xex::memory(bytes)
    {
        return (Cpu::M6502, memory, origin, starts, how);
    }
    if ending == "seg"
        && let Some((origin, starts)) = crate::media::xex::starts_from_name(name)
    {
        return (Cpu::M6502, bytes.to_vec(), origin, starts, format!("A segment of an Atari binary load file, {} bytes at ${origin:04X}", bytes.len()));
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
    number32(text).and_then(|n| u16::try_from(n).ok())
}

/// The same, up to 32 bits.
fn number32(text: &str) -> Option<u32> {
    number64(text).and_then(|n| u32::try_from(n).ok())
}

/// The same, up to 64 bits.
fn number64(text: &str) -> Option<u64> {
    let t = text.trim();
    let (digits, radix) = if let Some(h) = t.strip_prefix('$').or_else(|| t.strip_prefix('#')).or_else(|| t.strip_prefix("0x")).or_else(|| t.strip_prefix("0X")) {
        (h, 16)
    } else if let Some(h) = t.strip_suffix('h').or_else(|| t.strip_suffix('H')) {
        (h, 16)
    } else {
        (t, 10)
    };
    u64::from_str_radix(digits, radix).ok()
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
        // An x86 program (PE, ELF, MZ or .COM) is taken whole.
        if let Some(program) = crate::x86::recognise(&name, &bytes) {
            let found = program.what.clone();
            self.dialog = Dialog::Disassemble(Box::new(DisasmBox { cpu: Cpu::Z80, icl2900: false, x86: true, x86_bits: program.bits, x86_program: Some(program), z88: false, m68k_program: None, machine: crate::m68k_disasm::Machine::Plain, vcs: None, bytes, origin: "$0".into(), starts: String::new(), stem, found, error: String::new() }));
            return;
        }
        // An Atari 2600 cartridge: its banks, and how it switches them.
        if let Some(cart) = crate::atari2600::recognise(&name, &bytes) {
            let found = cart.describe();
            self.dialog = Dialog::Disassemble(Box::new(DisasmBox { cpu: Cpu::M6502, icl2900: false, x86: false, x86_program: None, x86_bits: 32, z88: false, m68k_program: None, machine: crate::m68k_disasm::Machine::Plain, vcs: Some(cart), bytes, origin: "$F000".into(), starts: String::new(), stem, found, error: String::new() }));
            return;
        }
        // A 68000 program (Amiga, Atari ST, a boot block, a Mega Drive cartridge, a Kickstart ROM) says the rest.
        if let Some(program) = crate::m68k_disasm::recognise(&name, &bytes) {
            let found = program.what.clone();
            let machine = program.machine;
            let origin = format!("${:X}", program.sections[0].base);
            let starts = program.starts.iter().map(|s| format!("${s:X}")).collect::<Vec<_>>().join(", ");
            self.dialog = Dialog::Disassemble(Box::new(DisasmBox { cpu: Cpu::M68k, icl2900: false, x86: false, x86_program: None, x86_bits: 32, z88: false, m68k_program: Some(program), machine, vcs: None, bytes, origin, starts, stem, found, error: String::new() }));
            return;
        }
        let (cpu, code, origin, starts, found) = guess(&name, &bytes, load_address);
        let starts = starts.iter().map(|s| format!("${s:04X}")).collect::<Vec<_>>().join(", ");
        self.dialog = Dialog::Disassemble(Box::new(DisasmBox { cpu, icl2900: false, x86: false, x86_program: None, x86_bits: 32, z88: false, m68k_program: None, machine: crate::m68k_disasm::Machine::Plain, vcs: None, bytes: code, origin: format!("${origin:04X}"), starts, stem, found, error: String::new() }));
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
                    for (cpu, name) in [(Cpu::Z80, "Z80 (and the Next's)"), (Cpu::M6502, "6502"), (Cpu::M6809, "6809"), (Cpu::M68k, "68000")] {
                        if ui.radio(!state.icl2900 && !state.x86 && state.cpu == cpu, name).clicked() {
                            (state.cpu, state.icl2900, state.x86) = (cpu, false, false);
                        }
                    }
                    if ui.radio(state.x86, "x86").clicked() {
                        (state.icl2900, state.x86) = (false, true);
                    }
                    if ui.radio(state.icl2900, "ICL 2900").clicked() {
                        (state.icl2900, state.x86) = (true, false);
                    }
                });
                ui.end_row();
                if !state.icl2900 && !state.x86 && state.cpu == Cpu::Z80 {
                    ui.label("");
                    ui.checkbox(&mut state.z88, "For a Cambridge Z88: name its operating system calls (RST $20 and the code after it)");
                    ui.end_row();
                }
                let vcs = !state.icl2900 && !state.x86 && state.cpu == Cpu::M6502 && state.vcs.is_some();
                if vcs && let Some(cart) = &mut state.vcs {
                    ui.label("Bank switching");
                    let size = cart.bytes.len();
                    egui::ComboBox::from_id_salt("vcs-scheme").selected_text(cart.scheme.name()).show_ui(ui, |ui| {
                        for scheme in crate::atari2600::Scheme::ALL.into_iter().filter(|s| crate::atari2600::Scheme::fits(*s, size)) {
                            ui.selectable_value(&mut cart.scheme, scheme, scheme.name());
                        }
                    });
                    ui.end_row();
                }
                let m68k = !state.icl2900 && !state.x86 && state.cpu == Cpu::M68k;
                if m68k && state.m68k_program.as_ref().is_none_or(|p| p.layout == crate::m68k_disasm::Layout::Raw) {
                    ui.label("Machine");
                    ui.horizontal(|ui| {
                        for machine in crate::m68k_disasm::Machine::ALL {
                            ui.radio_value(&mut state.machine, machine, machine.name()).on_hover_text("Whose hardware and system calls to name");
                        }
                    });
                    ui.end_row();
                }
                // A program the file was found to be says its own mode, addresses and starts.
                let whole = vcs || (state.x86 && state.x86_program.is_some()) || (m68k && state.m68k_program.as_ref().is_some_and(|p| p.layout != crate::m68k_disasm::Layout::Raw));
                if state.x86 && !whole {
                    ui.label("Mode");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut state.x86_bits, 16, "16-bit (8086, DOS)");
                        ui.radio_value(&mut state.x86_bits, 32, "32-bit");
                        ui.radio_value(&mut state.x86_bits, 64, "64-bit");
                    });
                    ui.end_row();
                }
                if !whole {
                    ui.label("Loads at");
                    ui.add(egui::TextEdit::singleline(&mut state.origin).desired_width(120.0));
                    ui.end_row();
                }
                if !state.icl2900 && !whole {
                    ui.label("Starts at");
                    ui.add(egui::TextEdit::singleline(&mut state.starts).hint_text("$8000, $8100...").desired_width(200.0));
                    ui.end_row();
                }
            });
            let how = if !state.icl2900 && !state.x86 && state.cpu == Cpu::M6502 && state.vcs.is_some() {
                "Each bank is followed from its reset vector, and from bank to bank where the code switches; what's never reached is data, and short runs of it are drawn as the pixels they'd be. The TIA's and RIOT's registers are named, each instruction's cycles are beside it, and code that runs past a scanline's 76 cycles is pointed out. It's written for DASM."
            } else if !state.icl2900 && !state.x86 && state.cpu == Cpu::M68k {
                "The code is followed from where it starts through every branch, jump and call; what it never reaches is kept as data. An Amiga program's hunks become sections and its relocated longs labels, so it assembles into a working program again; the Amiga's and ST's hardware, library calls and TOS calls are named."
            } else if state.x86 && state.x86_program.is_some() {
                "Its code is followed from where it's entered (and from what it exports or names), through every jump and call; what it never reaches is kept as data. Every x86 instruction up to today's (SSE, AVX, AVX-512, AMX) is known."
            } else if state.icl2900 {
                "Every halfword is read as an instruction, one after another, and listed in SFL with its byte address; what isn't an instruction is shown as it is. Jumps' targets get labels."
            } else {
                "The code is followed from where it starts, through every jump and call; what it never reaches is kept as data. Add more places it starts, comma between, for code reached some other way (interrupts, tables of addresses)."
            };
            ui.label(RichText::new(how).color(theme::text_faint()).size(11.5));
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
            if state.x86 {
                let program = match &state.x86_program {
                    Some(program) => program.clone(),
                    None => {
                        let Some(origin) = number64(&state.origin) else {
                            state.error = "Say where it loads: $401000, or 4198400.".into();
                            return true;
                        };
                        let starts: Option<Vec<u64>> = state.starts.split(',').filter(|s| !s.trim().is_empty()).map(number64).collect();
                        let Some(starts) = starts else {
                            state.error = "Say where it starts: addresses, comma between.".into();
                            return true;
                        };
                        crate::x86::raw(&state.bytes, state.x86_bits, origin, &starts)
                    }
                };
                let stem = state.stem.clone();
                self.dialog = Dialog::None;
                let source = crate::x86::disassemble(&program);
                let mut doc = Document::untitled(format!("{stem}.asm"), &PRESETS[0]);
                doc.text.set_content(source.lines().map(str::to_string).collect());
                highlight::load_now();
                let language = if program.bits == 64 { "Assembly x86_64" } else { "Assembly 80x86" };
                if let Some((_, syntax)) = highlight::engine().and_then(|e| e.names().iter().find(|(n, _)| n == language)) {
                    doc.set_language(Language::Named(*syntax));
                }
                doc.update_syntax();
                self.docs.push(doc);
                self.active = self.docs.len() - 1;
                self.pending_focus = true;
                return true;
            }
            if state.cpu == Cpu::M6502
                && let Some(cart) = state.vcs.clone()
            {
                let stem = state.stem.clone();
                self.dialog = Dialog::None;
                let source = crate::atari2600::disassemble(&cart);
                let mut doc = Document::untitled(format!("{stem}.asm"), &PRESETS[0]);
                doc.text.set_content(source.lines().map(str::to_string).collect());
                highlight::load_now();
                if let Some((_, syntax)) = highlight::engine().and_then(|e| e.names().iter().find(|(n, _)| n == "Assembly 6502")) {
                    doc.set_language(Language::Named(*syntax));
                }
                doc.update_syntax();
                self.docs.push(doc);
                self.active = self.docs.len() - 1;
                self.pending_focus = true;
                return true;
            }
            if state.cpu == Cpu::M68k {
                let program = match state.m68k_program.clone().filter(|p| p.layout != crate::m68k_disasm::Layout::Raw) {
                    Some(program) => program,
                    None => {
                        let Some(origin) = number32(&state.origin) else {
                            state.error = "Say where it loads: $40000, or 262144.".into();
                            return true;
                        };
                        let starts: Option<Vec<u32>> = state.starts.split(',').filter(|s| !s.trim().is_empty()).map(number32).collect();
                        let Some(starts) = starts.filter(|s| !s.is_empty()) else {
                            state.error = "Say where it starts: addresses, comma between.".into();
                            return true;
                        };
                        // A boot block, cartridge or ROM keeps what was found (its vectors); only where it is changes.
                        let mut program = crate::m68k_disasm::raw(&state.bytes, origin, &starts, state.machine);
                        if let Some(found) = state.m68k_program.as_ref().filter(|p| p.sections[0].base == origin) {
                            program.pointers = found.pointers.clone();
                            program.what = found.what.clone();
                        }
                        program
                    }
                };
                let stem = state.stem.clone();
                self.dialog = Dialog::None;
                let source = crate::m68k_disasm::disassemble(&program);
                let mut doc = Document::untitled(format!("{stem}.s"), &PRESETS[0]);
                doc.text.set_content(source.lines().map(str::to_string).collect());
                highlight::load_now();
                if let Some((_, syntax)) = highlight::engine().and_then(|e| e.names().iter().find(|(n, _)| n == "Assembly 680x0")) {
                    doc.set_language(Language::Named(*syntax));
                }
                doc.update_syntax();
                self.docs.push(doc);
                self.active = self.docs.len() - 1;
                self.pending_focus = true;
                return true;
            }
            if state.icl2900 {
                let Some(origin) = number32(&state.origin) else {
                    state.error = "Say where it loads, as a byte address: $8000, or 32768.".into();
                    return true;
                };
                let listing = crate::icl2900::disassemble(&state.bytes, origin);
                let stem = state.stem.clone();
                self.dialog = Dialog::None;
                let mut doc = Document::untitled(format!("{stem}.sfl"), &PRESETS[0]);
                doc.text.set_content(listing.lines().map(str::to_string).collect());
                self.docs.push(doc);
                self.active = self.docs.len() - 1;
                self.pending_focus = true;
                return true;
            }
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
            let (cpu, stem, z88) = (state.cpu, state.stem.clone(), state.z88);
            let source = crate::disasm::disassemble_checked_for(cpu, &state.bytes, origin, &starts, z88);
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
        // An Atari binary load file: a routine at $0600 called as it loads, the program at $2000 run once loaded.
        let mut xex = vec![0xFF, 0xFF, 0x00, 0x06, 0x00, 0x06, 0x60, 0xE2, 0x02, 0xE3, 0x02, 0x00, 0x06];
        xex.extend([0x00, 0x20, 0x02, 0x20, 0x4C, 0x00, 0x20, 0xE0, 0x02, 0xE1, 0x02, 0x00, 0x20]);
        let (cpu, code, origin, starts, _) = guess("GAME.XEX", &xex, None);
        assert_eq!((cpu, origin, starts, code.len()), (Cpu::M6502, 0x0600, vec![0x0600, 0x2000], 0x2003 - 0x0600));
        let (cpu, code, origin, starts, _) = guess("GAME.XEX \u{203A} segment 3 $2000-$2002 run $2000.seg", &[0x4C, 0x00, 0x20], None);
        assert_eq!((cpu, origin, starts, code.len()), (Cpu::M6502, 0x2000, vec![0x2000], 3));
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
