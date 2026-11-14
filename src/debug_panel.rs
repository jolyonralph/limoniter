//! The debugger's panel, wherever a Z80 program is stopped (the built-in Spectrum, ZEsarUX) and as far as it
//! goes for the 6502 in VICE: where it stopped and why, the registers, the call stack, watches, a window on
//! memory, and the breakpoints with the conditions they stop on and the addresses whose writing stops it.
//!
//! Watches and conditions are expressions: registers (A, HL, IX, SP...), flags (ZF, CF, SF, PF, HF, NF), labels,
//! numbers as the assembler writes them ($8000, 0x8000, #8000, %1010, 32768), `[addr]` for the byte at an
//! address and `word[addr]` for the word, the usual arithmetic, and comparisons joined with && and ||.

use crate::asm::Assembly;
use crate::remote_debug::Stop;
use crate::text_buffer::CodeMarks;
use crate::theme;
use crate::zx_machine::Registers;
use eframe::egui::{self, RichText, Ui};
use std::collections::BTreeMap;

/// What the panel asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    CarryOn,
    Step,
    StepOver,
}

/// What an expression can see: registers, memory and labels.
pub struct Seen<'a> {
    pub registers: Option<&'a Registers>,
    pub peek: &'a dyn Fn(u16) -> u8,
    pub labels: Option<&'a BTreeMap<String, i64>>,
}

/// Works out an expression.
pub fn eval(text: &str, seen: &Seen) -> Result<i64, String> {
    let mut parser = Parser { chars: text.chars().collect(), at: 0, seen };
    let value = parser.or()?;
    parser.space();
    if parser.at < parser.chars.len() {
        return Err(format!("\"{}\" wasn't understood", parser.chars[parser.at..].iter().collect::<String>()));
    }
    Ok(value)
}

struct Parser<'a, 'b> {
    chars: Vec<char>,
    at: usize,
    seen: &'a Seen<'b>,
}

impl Parser<'_, '_> {
    fn space(&mut self) {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
    }

    /// Whether the text goes on with a word (case aside) or symbol, taking it if so. A word must end there.
    fn take(&mut self, what: &str) -> bool {
        self.space();
        let n = what.chars().count();
        let here: String = self.chars.iter().skip(self.at).take(n).collect();
        if !here.eq_ignore_ascii_case(what) {
            return false;
        }
        let wordy = what.chars().all(|c| c.is_ascii_alphabetic());
        if wordy && self.chars.get(self.at + n).is_some_and(|c| c.is_alphanumeric() || *c == '_') {
            return false;
        }
        self.at += n;
        true
    }

    fn or(&mut self) -> Result<i64, String> {
        let mut value = self.and()?;
        while self.take("||") || self.take("or") {
            let right = self.and()?;
            value = i64::from(value != 0 || right != 0);
        }
        Ok(value)
    }

    fn and(&mut self) -> Result<i64, String> {
        let mut value = self.compare()?;
        while self.take("&&") || self.take("and") {
            let right = self.compare()?;
            value = i64::from(value != 0 && right != 0);
        }
        Ok(value)
    }

    fn compare(&mut self) -> Result<i64, String> {
        let left = self.bits()?;
        for (op, test) in [("==", 0), ("!=", 1), ("<>", 1), ("<=", 2), (">=", 3), ("=", 0), ("<", 4), (">", 5)] {
            // << and >> are shifts, not comparisons.
            if (op == "<" && self.chars.get(self.at..self.at + 2).is_some_and(|c| c == ['<', '<'])) || (op == ">" && self.chars.get(self.at..self.at + 2).is_some_and(|c| c == ['>', '>'])) {
                continue;
            }
            if self.take(op) {
                let right = self.bits()?;
                return Ok(i64::from(match test {
                    0 => left == right,
                    1 => left != right,
                    2 => left <= right,
                    3 => left >= right,
                    4 => left < right,
                    _ => left > right,
                }));
            }
        }
        Ok(left)
    }

    fn bits(&mut self) -> Result<i64, String> {
        let mut value = self.sum()?;
        loop {
            self.space();
            let next_two: String = self.chars.iter().skip(self.at).take(2).collect();
            if next_two == "&&" || next_two == "||" {
                return Ok(value);
            }
            if self.take("&") {
                value &= self.sum()?;
            } else if self.take("|") {
                value |= self.sum()?;
            } else if self.take("^") {
                value ^= self.sum()?;
            } else if self.take("<<") {
                value <<= self.sum()?.clamp(0, 63);
            } else if self.take(">>") {
                value >>= self.sum()?.clamp(0, 63);
            } else {
                return Ok(value);
            }
        }
    }

    fn sum(&mut self) -> Result<i64, String> {
        let mut value = self.product()?;
        loop {
            if self.take("+") {
                value += self.product()?;
            } else if self.take("-") {
                value -= self.product()?;
            } else {
                return Ok(value);
            }
        }
    }

    fn product(&mut self) -> Result<i64, String> {
        let mut value = self.unary()?;
        loop {
            if self.take("*") {
                value *= self.unary()?;
            } else if self.take("/") {
                let by = self.unary()?;
                value = value.checked_div(by).ok_or("division by 0")?;
            } else if self.take("%") && !self.chars.get(self.at).is_some_and(|c| *c == '0' || *c == '1') {
                let by = self.unary()?;
                value = value.checked_rem(by).ok_or("division by 0")?;
            } else {
                return Ok(value);
            }
        }
    }

    fn unary(&mut self) -> Result<i64, String> {
        if self.take("-") {
            return Ok(-self.unary()?);
        }
        if self.take("!") || self.take("not") {
            return Ok(i64::from(self.unary()? == 0));
        }
        if self.take("~") {
            return Ok(!self.unary()?);
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<i64, String> {
        self.space();
        if self.take("(") {
            let value = self.or()?;
            if !self.take(")") {
                return Err("a ( has no )".into());
            }
            return Ok(value);
        }
        let word = self.take("word[");
        if word || self.take("[") {
            let address = self.or()?;
            if !self.take("]") {
                return Err("a [ has no ]".into());
            }
            let peek = self.seen.peek;
            let low = i64::from(peek(address as u16));
            return Ok(if word { low | i64::from(peek((address as u16).wrapping_add(1))) << 8 } else { low });
        }
        let start = self.at;
        let c = *self.chars.get(self.at).ok_or("something is missing at the end")?;
        // Numbers, written as the assemblers write them.
        let radix_from = |s: &mut Self, radix: u32, skip: usize| -> Result<i64, String> {
            s.at += skip;
            let from = s.at;
            while s.chars.get(s.at).is_some_and(|c| c.is_digit(radix) || *c == '_') {
                s.at += 1;
            }
            let digits: String = s.chars[from..s.at].iter().filter(|c| **c != '_').collect();
            i64::from_str_radix(&digits, radix).map_err(|_| "a number wasn't understood".to_string())
        };
        if c == '$' || c == '#' {
            return radix_from(self, 16, 1);
        }
        if c == '%' {
            return radix_from(self, 2, 1);
        }
        if c == '0' && matches!(self.chars.get(self.at + 1), Some('x' | 'X')) {
            return radix_from(self, 16, 2);
        }
        if c.is_ascii_digit() {
            // 8000h is hex too.
            let mut end = self.at;
            while self.chars.get(end).is_some_and(|c| c.is_ascii_hexdigit()) {
                end += 1;
            }
            if matches!(self.chars.get(end), Some('h' | 'H')) && !self.chars.get(end + 1).is_some_and(|c| c.is_alphanumeric()) {
                let value = radix_from(self, 16, 0)?;
                self.at += 1;
                return Ok(value);
            }
            return radix_from(self, 10, 0);
        }
        if c.is_alphabetic() || matches!(c, '_' | '.' | '@') {
            while self.chars.get(self.at).is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '@' | '\'')) {
                self.at += 1;
            }
            let name: String = self.chars[start..self.at].iter().collect();
            if let Some(value) = self.seen.registers.and_then(|r| register(r, &name)) {
                return Ok(value);
            }
            if let Some(value) = self.seen.labels.and_then(|labels| labels.get(&name).or_else(|| labels.iter().find(|(k, _)| k.eq_ignore_ascii_case(&name)).map(|(_, v)| v))) {
                return Ok(*value);
            }
            return Err(format!("{name} isn't a register or a label"));
        }
        Err(format!("\"{c}\" wasn't understood"))
    }
}

/// A Z80 register or flag by name.
fn register(r: &Registers, name: &str) -> Option<i64> {
    let high = |w: u16| i64::from(w >> 8);
    let low = |w: u16| i64::from(w & 0xFF);
    let flag = |bit: u8| i64::from(r.af & (1 << bit) != 0);
    Some(match name.to_ascii_uppercase().as_str() {
        "A" => high(r.af),
        "F" => low(r.af),
        "B" => high(r.bc),
        "C" => low(r.bc),
        "D" => high(r.de),
        "E" => low(r.de),
        "H" => high(r.hl),
        "L" => low(r.hl),
        "AF" => i64::from(r.af),
        "BC" => i64::from(r.bc),
        "DE" => i64::from(r.de),
        "HL" => i64::from(r.hl),
        "IX" => i64::from(r.ix),
        "IY" => i64::from(r.iy),
        "IXH" => high(r.ix),
        "IXL" => low(r.ix),
        "IYH" => high(r.iy),
        "IYL" => low(r.iy),
        "SP" => i64::from(r.sp),
        "PC" => i64::from(r.pc),
        "I" => i64::from(r.i),
        "R" => i64::from(r.r),
        "AF'" => i64::from(r.af2),
        "BC'" => i64::from(r.bc2),
        "DE'" => i64::from(r.de2),
        "HL'" => i64::from(r.hl2),
        "CF" => flag(0),
        "NF" => flag(1),
        "PF" | "VF" => flag(2),
        "HF" => flag(4),
        "ZF" => flag(6),
        "SF" => flag(7),
        _ => return None,
    })
}

/// An address as a label and how far past it: "frame+3". Only labels within 256 bytes before it count.
pub fn label_of(address: u16, assembly: &Assembly) -> Option<String> {
    let address = i64::from(address);
    let (name, value) = assembly.definitions.iter().filter(|d| d.written == d.name).filter_map(|d| assembly.symbols.get(&d.name).map(|v| (&d.name, *v))).filter(|(_, v)| *v <= address && address - *v < 256).max_by_key(|(_, v)| *v)?;
    Some(if value == address { name.clone() } else { format!("{name}+{}", address - value) })
}

/// One call on the stack: the address it will return to, and the call it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Where on the stack the return address is.
    pub at: u16,
    pub returns_to: u16,
    /// The CALL or RST's own address.
    pub call: u16,
}

/// The calls a Z80 program is inside, innermost first, found by looking down the stack for words just after a
/// CALL or RST. A word pushed by PUSH that happens to look like one can be taken for one.
pub fn call_stack(sp: u16, peek: &dyn Fn(u16) -> u8) -> Vec<Frame> {
    let mut frames = Vec::new();
    let mut at = sp;
    for _ in 0..64 {
        if frames.len() >= 12 || at >= 0xFFFE {
            break;
        }
        let word = u16::from_le_bytes([peek(at), peek(at.wrapping_add(1))]);
        let op3 = peek(word.wrapping_sub(3));
        let op1 = peek(word.wrapping_sub(1));
        // CALL nn and CALL cc,nn; or RST n.
        let call = if word >= 3 && (op3 == 0xCD || op3 & 0xC7 == 0xC4) {
            Some(word.wrapping_sub(3))
        } else if word >= 1 && op1 & 0xC7 == 0xC7 {
            Some(word.wrapping_sub(1))
        } else {
            None
        };
        if let Some(call) = call {
            frames.push(Frame { at, returns_to: word, call });
        }
        at = at.wrapping_add(2);
    }
    frames
}

/// Where an address is in the source, as words: "line 12" or "line 4 of lib.asm", with its label.
fn place_of(address: u16, assembly: Option<&Assembly>) -> String {
    let Some(assembly) = assembly.filter(|a| a.line_at(i64::from(address)).is_some() || address >= 0x4000) else {
        return if address < 0x4000 { format!("${address:04X} in the ROM") } else { format!("${address:04X}") };
    };
    let line = assembly.line_at(i64::from(address)).map(|(file, line)| {
        let of = if file == 0 { String::new() } else { assembly.files.get(file).and_then(|f| f.file_name()).map(|n| format!(" of {}", n.to_string_lossy())).unwrap_or_default() };
        format!(", line {}{of}", line + 1)
    });
    let label = label_of(address, assembly).map(|l| format!(" {l}")).unwrap_or_default();
    format!("${address:04X}{label}{}", line.unwrap_or_default())
}

/// The panel while a program is stopped. `memory` is all 64K as it is now; `before`, as it was at the last stop,
/// for what's changed to stand out.
pub fn stopped(ui: &mut Ui, stop: &Stop, memory: Option<&[u8]>, before: Option<&[u8]>, assembly: Option<&Assembly>, code: &mut CodeMarks) -> Option<Action> {
    let pc = stop.pc;
    let mut action = None;
    let mono = |text: String| RichText::new(text).monospace().size(12.0);
    ui.add_space(4.0);
    egui::Frame::new().fill(ui.visuals().faint_bg_color).inner_margin(egui::Margin::same(6)).corner_radius(4.0).show(ui, |ui| {
        let place = match assembly.and_then(|a| a.line_at(pc as i64).map(|(file, line)| (a, file, line))) {
            Some((a, file, line)) => {
                let name = a.files.get(file).and_then(|f| f.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                format!("Stopped at ${pc:04X}, line {} of {name}", line + 1)
            }
            None if stop.z80.is_some() && pc < 0x4000 => format!("Stopped at ${pc:04X}, in the ROM"),
            None => format!("Stopped at ${pc:04X}"),
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(place).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Step over").on_hover_text("Runs this instruction, or the whole of a subroutine it calls, and stops after it").clicked() {
                    action = Some(Action::StepOver);
                }
                if ui.small_button("Step").on_hover_text("Runs one instruction, into a CALL").clicked() {
                    action = Some(Action::Step);
                }
                if ui.small_button("Carry on").on_hover_text("Runs on until the next breakpoint").clicked() {
                    action = Some(Action::CarryOn);
                }
            });
        });
        if let Some(reason) = &stop.reason {
            ui.label(RichText::new(reason).color(theme::accent()).size(12.0));
        }
        let Some(r) = stop.z80 else {
            // Another processor's registers, as its emulator gives them.
            ui.label(mono(stop.words.join("   ")));
            return;
        };
        let flags = r.af as u8;
        let flag_names: String = [(7, 'S'), (6, 'Z'), (4, 'H'), (2, 'P'), (1, 'N'), (0, 'C')].iter().map(|(bit, name)| if flags & (1 << bit) != 0 { *name } else { '-' }).collect();
        let top = memory.map(|m| u16::from_le_bytes([m[r.sp as usize], m[r.sp.wrapping_add(1) as usize]]));
        egui::Grid::new("z80-registers").num_columns(4).spacing([14.0, 2.0]).show(ui, |ui| {
            for row in [
                [format!("AF {:04X}", r.af), format!("BC {:04X}", r.bc), format!("DE {:04X}", r.de), format!("HL {:04X}", r.hl)],
                [format!("AF' {:04X}", r.af2), format!("BC' {:04X}", r.bc2), format!("DE' {:04X}", r.de2), format!("HL' {:04X}", r.hl2)],
                [format!("IX {:04X}", r.ix), format!("IY {:04X}", r.iy), format!("SP {:04X}", r.sp), format!("PC {:04X}", r.pc)],
                [format!("I {:02X}  R {:02X}", r.i, r.r), format!("IM {}  {}", r.im, if r.iff { "EI" } else { "DI" }), format!("Flags {flag_names}"), top.map(|top| format!("(SP) {top:04X}")).unwrap_or_default()],
            ] {
                for cell in row {
                    ui.label(mono(cell));
                }
                ui.end_row();
            }
        });
        let Some(memory) = memory else {
            return;
        };
        let peek = |a: u16| memory[a as usize];
        // The calls it's inside.
        let frames = call_stack(r.sp, &peek);
        egui::CollapsingHeader::new(format!("Call stack ({})", frames.len())).id_salt("debug-calls").default_open(true).show(ui, |ui| {
            ui.label(mono(format!("now   {}", place_of(pc, assembly))));
            for frame in &frames {
                ui.label(mono(format!("from  {}", place_of(frame.call, assembly)))).on_hover_text(format!("Returns to ${:04X}; the address is on the stack at ${:04X}", frame.returns_to, frame.at));
            }
            if frames.is_empty() {
                ui.label(RichText::new("Not inside a CALL that can be seen.").color(theme::text_faint()).size(11.5));
            }
        });
        let labels = assembly.map(|a| &a.symbols);
        let seen = Seen { registers: Some(&r), peek: &peek, labels };
        // Watches: expressions worked out at every stop.
        egui::CollapsingHeader::new("Watches").id_salt("debug-watches").default_open(true).show(ui, |ui| {
            let mut gone = None;
            for (index, watch) in code.watches.iter().enumerate() {
                ui.horizontal(|ui| {
                    if crate::access::called(ui.small_button("\u{D7}"), "Stop watching this").on_hover_text("Stop watching this").clicked() {
                        gone = Some(index);
                    }
                    let value = match eval(watch, &seen) {
                        Ok(v) => format!("{watch} = {v} (${:X})", v & 0xFFFF_FFFF),
                        Err(problem) => format!("{watch}: {problem}"),
                    };
                    ui.label(mono(value));
                });
            }
            if let Some(index) = gone {
                code.watches.remove(index);
            }
            ui.horizontal(|ui| {
                let field = ui.add(egui::TextEdit::singleline(&mut code.new_watch).hint_text("A, HL, [counter], word[ptr]...").desired_width(200.0));
                if (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.small_button("Watch").clicked() {
                    let watch = code.new_watch.trim().to_string();
                    if !watch.is_empty() {
                        code.watches.push(watch);
                        code.new_watch.clear();
                    }
                }
            });
        });
        // A window on memory, at an address, a label, or anything an expression gives.
        egui::CollapsingHeader::new("Memory").id_salt("debug-memory").default_open(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("At");
                ui.add(egui::TextEdit::singleline(&mut code.memory_at).hint_text("HL, $5800, a label...").desired_width(160.0));
            });
            let shown = if code.memory_at.trim().is_empty() { Ok(i64::from(r.hl)) } else { eval(&code.memory_at, &seen) };
            match shown {
                Ok(start) => {
                    let start = (start as u16) & 0xFFF0;
                    for row in 0..8u16 {
                        let address = start.wrapping_add(row * 16);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(mono(format!("{address:04X}")).color(theme::text_dim()));
                            for i in 0..16u16 {
                                let at = address.wrapping_add(i);
                                let byte = peek(at);
                                let changed = before.is_some_and(|b| b[at as usize] != byte);
                                let text = mono(format!("{byte:02X}"));
                                ui.label(if changed { text.color(theme::accent()).strong() } else { text }).on_hover_text(format!("${at:04X} = {byte}"));
                            }
                            let chars: String = (0..16u16).map(|i| peek(address.wrapping_add(i))).map(|b| if (32..127).contains(&b) { b as char } else { '.' }).collect();
                            ui.label(mono(chars).color(theme::text_dim()));
                        });
                    }
                }
                Err(problem) => {
                    ui.label(RichText::new(problem).color(theme::problem_mark()).size(11.5));
                }
            }
        });
    });
    action
}

/// The breakpoints, each with the condition it stops on, and the addresses whose writing stops it: shown whether
/// it's running or stopped, so they can be set up first. `lines` gives a line's text.
pub fn breakpoints(ui: &mut Ui, code: &mut CodeMarks, lines: &dyn Fn(usize) -> String, conditions: bool, writes_supported: bool) {
    egui::CollapsingHeader::new(format!("Breakpoints ({})", code.breakpoints.len())).id_salt("debug-breakpoints").default_open(false).show(ui, |ui| {
        if code.breakpoints.is_empty() {
            ui.label(RichText::new("Click beside a line's number for a breakpoint.").color(theme::text_faint()).size(11.5));
        }
        let lines_with: Vec<usize> = code.breakpoints.iter().copied().collect();
        for line in lines_with {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{:>4}: {}", line + 1, lines(line).trim())).monospace().size(11.5));
            });
            if !conditions {
                continue;
            }
            let condition = code.conditions.entry(line).or_default();
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.label(RichText::new("only when").size(11.5).color(theme::text_dim()));
                ui.add(egui::TextEdit::singleline(condition).hint_text("always, or e.g. A=0 && [count]>3").desired_width(220.0));
            });
        }
        code.conditions.retain(|line, text| code.breakpoints.contains(line) && !text.trim().is_empty());
        ui.add_space(4.0);
        if !writes_supported {
            return;
        }
        ui.label(RichText::new("Stop when written to").size(12.0));
        let mut gone = None;
        for (index, watch) in code.write_watches.iter().enumerate() {
            ui.horizontal(|ui| {
                if crate::access::called(ui.small_button("\u{D7}"), "Stop watching this").clicked() {
                    gone = Some(index);
                }
                ui.label(RichText::new(watch).monospace().size(11.5));
            });
        }
        if let Some(index) = gone {
            code.write_watches.remove(index);
        }
        ui.horizontal(|ui| {
            let field = ui.add(egui::TextEdit::singleline(&mut code.new_write_watch).hint_text("An address or a label").desired_width(160.0));
            if (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.small_button("Add").clicked() {
                let watch = code.new_write_watch.trim().to_string();
                if !watch.is_empty() {
                    code.write_watches.push(watch);
                    code.new_write_watch.clear();
                }
            }
        });
    });
}

/// The addresses whose writing stops it, from what's written in the list: a label, or a range as label..label+n.
pub fn write_addresses(code: &CodeMarks, labels: Option<&BTreeMap<String, i64>>) -> Vec<u16> {
    let none = |_: u16| 0;
    let seen = Seen { registers: None, peek: &none, labels };
    let mut found = Vec::new();
    for watch in &code.write_watches {
        match watch.split_once("..") {
            Some((from, to)) => {
                if let (Ok(from), Ok(to)) = (eval(from, &seen), eval(to, &seen)) {
                    found.extend((from..=to.min(from + 0x4000)).map(|a| a as u16));
                }
            }
            None => found.extend(eval(watch, &seen).ok().map(|a| a as u16)),
        }
    }
    found
}

/// The conditions on breakpoints, by address.
pub fn conditions_by_address(code: &CodeMarks, assembly: &Assembly) -> BTreeMap<u16, String> {
    code.conditions.iter().filter_map(|(line, text)| assembly.address_of(0, *line).map(|a| (a as u16, text.clone()))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expressions_see_registers_flags_labels_and_memory() {
        let r = Registers { af: 0x0741, bc: 0x1234, hl: 0x8000, sp: 0xFF00, ..Registers::default() };
        let mut memory = vec![0u8; 65536];
        memory[0x8000] = 0x2A;
        memory[0x8001] = 0x01;
        let labels: BTreeMap<String, i64> = [("count".to_string(), 0x8000), ("main.loop".to_string(), 0x8010)].into();
        let peek = |a: u16| memory[a as usize];
        let seen = Seen { registers: Some(&r), peek: &peek, labels: Some(&labels) };
        let v = |t: &str| eval(t, &seen);
        assert_eq!(v("A"), Ok(7));
        assert_eq!(v("a = 7"), Ok(1));
        assert_eq!(v("ZF"), Ok(1), "F is 41: zero and carry set");
        assert_eq!(v("CF && !SF"), Ok(1));
        assert_eq!(v("BC"), Ok(0x1234));
        assert_eq!(v("C == $34 and B <> 0"), Ok(1));
        assert_eq!(v("[HL]"), Ok(0x2A));
        assert_eq!(v("word[count]"), Ok(0x012A));
        assert_eq!(v("[count] > 3 || A=0"), Ok(1));
        assert_eq!(v("main.loop - count"), Ok(16));
        assert_eq!(v("8000h + %11 + 0x10 + #10"), Ok(0x8000 + 3 + 16 + 16));
        assert_eq!(v("1 << 4 | 1"), Ok(17));
        assert_eq!(v("HL'"), Ok(0));
        assert!(v("nowhere").is_err());
        assert!(v("[HL").is_err());
    }

    #[test]
    fn the_call_stack_is_read_from_return_addresses() {
        let mut memory = vec![0u8; 65536];
        // CALL $9000 at $8003, so $8006 is on the stack; under it a pushed $1234 that follows no CALL; then
        // RST $10 at $9005, returning to $9006.
        memory[0x8003] = 0xCD;
        memory[0x9005] = 0xD7;
        for (at, word) in [(0xFF00u16, 0x9006u16), (0xFF02, 0x1234), (0xFF04, 0x8006)] {
            memory[at as usize..at as usize + 2].copy_from_slice(&word.to_le_bytes());
        }
        let frames = call_stack(0xFF00, &|a| memory[a as usize]);
        assert_eq!(frames.iter().map(|f| (f.returns_to, f.call)).collect::<Vec<_>>(), [(0x9006, 0x9005), (0x8006, 0x8003)]);
    }

    #[test]
    fn addresses_are_named_by_the_label_before_them() {
        let assembly = crate::asm::assemble(&crate::asm::z80::Z80, std::path::Path::new("t.asm"), "  org $8000\nmain: nop\n  nop\n.loop: nop\nnext: ret\n", &|_| Err("no".into()));
        assert_eq!(label_of(0x8000, &assembly).as_deref(), Some("main"));
        assert_eq!(label_of(0x8002, &assembly).as_deref(), Some("main+2"), "local labels aren't used for this");
        assert_eq!(label_of(0x8003, &assembly).as_deref(), Some("next"));
        assert_eq!(label_of(0x7000, &assembly), None);
    }
}
