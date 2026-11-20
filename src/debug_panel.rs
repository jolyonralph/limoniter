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

/// What an expression can see on a processor with 32-bit addresses and big-endian words, as the 68000 has:
/// registers (and anything else) by name, and memory.
pub struct Wide<'a> {
    pub names: &'a dyn Fn(&str) -> Option<i64>,
    pub peek: &'a dyn Fn(u32) -> u8,
}

/// Works out an expression.
pub fn eval(text: &str, seen: &Seen) -> Result<i64, String> {
    parse(text, seen, None)
}

/// Works out an expression for a 68000: its registers by name, labels, `[addr]` for a byte, `word[addr]` and
/// `long[addr]`, big-endian.
pub fn eval_wide(text: &str, wide: &Wide, labels: Option<&BTreeMap<String, i64>>) -> Result<i64, String> {
    let none = |_: u16| 0;
    let seen = Seen { registers: None, peek: &none, labels };
    parse(text, &seen, Some(wide))
}

fn parse(text: &str, seen: &Seen, wide: Option<&Wide>) -> Result<i64, String> {
    let mut parser = Parser { chars: text.chars().collect(), at: 0, seen, wide };
    let value = parser.or()?;
    parser.space();
    if parser.at < parser.chars.len() {
        return Err(crate::i18n::tf!("\"{}\" wasn't understood", parser.chars[parser.at..].iter().collect::<String>()));
    }
    Ok(value)
}

struct Parser<'a, 'b> {
    chars: Vec<char>,
    at: usize,
    seen: &'a Seen<'b>,
    wide: Option<&'a Wide<'a>>,
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
                value = value.checked_div(by).ok_or(crate::i18n::tr("division by 0"))?;
            } else if self.take("%") && !self.chars.get(self.at).is_some_and(|c| *c == '0' || *c == '1') {
                let by = self.unary()?;
                value = value.checked_rem(by).ok_or(crate::i18n::tr("division by 0"))?;
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
                return Err(crate::i18n::tr("a ( has no )").into());
            }
            return Ok(value);
        }
        let long = self.take("long[");
        let word = !long && self.take("word[");
        if long || word || self.take("[") {
            let address = self.or()?;
            if !self.take("]") {
                return Err(crate::i18n::tr("a [ has no ]").into());
            }
            if let Some(wide) = self.wide {
                let byte = |i: u32| i64::from((wide.peek)((address as u32).wrapping_add(i)));
                return Ok(if long {
                    byte(0) << 24 | byte(1) << 16 | byte(2) << 8 | byte(3)
                } else if word {
                    byte(0) << 8 | byte(1)
                } else {
                    byte(0)
                });
            }
            if long {
                return Err(crate::i18n::tr("long[...] is for the 68000").into());
            }
            let peek = self.seen.peek;
            let low = i64::from(peek(address as u16));
            return Ok(if word { low | i64::from(peek((address as u16).wrapping_add(1))) << 8 } else { low });
        }
        let start = self.at;
        let c = *self.chars.get(self.at).ok_or(crate::i18n::tr("something is missing at the end"))?;
        // Numbers, written as the assemblers write them.
        let radix_from = |s: &mut Self, radix: u32, skip: usize| -> Result<i64, String> {
            s.at += skip;
            let from = s.at;
            while s.chars.get(s.at).is_some_and(|c| c.is_digit(radix) || *c == '_') {
                s.at += 1;
            }
            let digits: String = s.chars[from..s.at].iter().filter(|c| **c != '_').collect();
            i64::from_str_radix(&digits, radix).map_err(|_| crate::i18n::tr("a number wasn't understood").to_string())
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
            if let Some(value) = self.wide.and_then(|wide| (wide.names)(&name)) {
                return Ok(value);
            }
            if let Some(value) = self.seen.registers.and_then(|r| register(r, &name)) {
                return Ok(value);
            }
            if let Some(value) = self.seen.labels.and_then(|labels| labels.get(&name).or_else(|| labels.iter().find(|(k, _)| k.eq_ignore_ascii_case(&name)).map(|(_, v)| v))) {
                return Ok(*value);
            }
            return Err(crate::i18n::tf!("{name} isn't a register or a label"));
        }
        Err(crate::i18n::tf!("\"{c}\" wasn't understood"))
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
    label_at(i64::from(address), assembly)
}

/// The same, for an address of any width.
pub fn label_at(address: i64, assembly: &Assembly) -> Option<String> {
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
        return if address < 0x4000 { crate::i18n::tf!("${address:04X} in the ROM") } else { format!("${address:04X}") };
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
                crate::i18n::tf!("Stopped at ${pc:04X}, line {} of {name}", line + 1)
            }
            None if stop.z80.is_some() && pc < 0x4000 => crate::i18n::tf!("Stopped at ${pc:04X}, in the ROM"),
            None => crate::i18n::tf!("Stopped at ${pc:04X}"),
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(place).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(crate::i18n::tr("Step over")).on_hover_text(crate::i18n::tr("Runs this instruction, or the whole of a subroutine it calls, and stops after it")).clicked() {
                    action = Some(Action::StepOver);
                }
                if ui.small_button(crate::i18n::tr("Step")).on_hover_text(crate::i18n::tr("Runs one instruction, into a CALL")).clicked() {
                    action = Some(Action::Step);
                }
                if ui.small_button(crate::i18n::tr("Carry on")).on_hover_text(crate::i18n::tr("Runs on until the next breakpoint")).clicked() {
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
        egui::CollapsingHeader::new(crate::i18n::tf!("Call stack ({})", frames.len())).id_salt("debug-calls").default_open(true).show(ui, |ui| {
            ui.label(mono(format!("now   {}", place_of(pc, assembly))));
            for frame in &frames {
                ui.label(mono(format!("from  {}", place_of(frame.call, assembly)))).on_hover_text(crate::i18n::tf!("Returns to ${:04X}; the address is on the stack at ${:04X}", frame.returns_to, frame.at));
            }
            if frames.is_empty() {
                ui.label(RichText::new(crate::i18n::tr("Not inside a CALL that can be seen.")).color(theme::text_faint()).size(11.5));
            }
        });
        let labels = assembly.map(|a| &a.symbols);
        let seen = Seen { registers: Some(&r), peek: &peek, labels };
        // Watches: expressions worked out at every stop.
        watches_section(ui, code, &|watch| eval(watch, &seen), "A, HL, [counter], word[ptr]...");
        // A window on memory, at an address, a label, or anything an expression gives.
        memory_section(ui, &mut code.memory_at, "HL, $5800, a label...", &|text| if text.trim().is_empty() { Ok(i64::from(r.hl)) } else { eval(text, &seen) }, 4, &|a| Some(peek(a as u16)), &|a| before.map(|b| b[(a & 0xFFFF) as usize]));
    });
    action
}

/// The watches: expressions worked out at every stop, with a field for another.
fn watches_section(ui: &mut Ui, code: &mut CodeMarks, value: &dyn Fn(&str) -> Result<i64, String>, hint: &str) {
    let mono = |text: String| RichText::new(text).monospace().size(12.0);
    egui::CollapsingHeader::new(crate::i18n::tr("Watches")).id_salt("debug-watches").default_open(true).show(ui, |ui| {
        let mut gone = None;
        for (index, watch) in code.watches.iter().enumerate() {
            ui.horizontal(|ui| {
                if crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Stop watching this").on_hover_text(crate::i18n::tr("Stop watching this")).clicked() {
                    gone = Some(index);
                }
                let value = match value(watch) {
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
            let field = ui.add(egui::TextEdit::singleline(&mut code.new_watch).hint_text(hint).desired_width(200.0));
            if (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.small_button(crate::i18n::tr("Watch")).clicked() {
                let watch = code.new_watch.trim().to_string();
                if !watch.is_empty() {
                    code.watches.push(watch);
                    code.new_watch.clear();
                }
            }
        });
    });
}

/// A window on memory, at what `start` makes of the address typed: 128 bytes, with those changed since the stop
/// before standing out, and those not read shown as --. `digits` is how wide addresses are written. Gives where
/// it starts, if the address could be worked out.
pub fn memory_section(ui: &mut Ui, typed: &mut String, hint: &str, start: &dyn Fn(&str) -> Result<i64, String>, digits: usize, peek: &dyn Fn(u32) -> Option<u8>, before: &dyn Fn(u32) -> Option<u8>) -> Option<u32> {
    let mono = |text: String| RichText::new(text).monospace().size(12.0);
    let mask = if digits <= 4 { 0xFFFF } else { 0xFFFF_FFFF };
    let mut shown_at = None;
    egui::CollapsingHeader::new(crate::i18n::tr("Memory")).id_salt("debug-memory").default_open(true).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("At"));
            ui.add(egui::TextEdit::singleline(typed).hint_text(hint).desired_width(160.0));
        });
        match start(typed) {
            Ok(start) => {
                let start = (start as u32) & mask & !0xF;
                shown_at = Some(start);
                for row in 0..8u32 {
                    let address = start.wrapping_add(row * 16) & mask;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label(mono(format!("{address:0digits$X}")).color(theme::text_dim()));
                        for i in 0..16u32 {
                            let at = address.wrapping_add(i) & mask;
                            match peek(at) {
                                Some(byte) => {
                                    let changed = before(at).is_some_and(|b| b != byte);
                                    let text = mono(format!("{byte:02X}"));
                                    ui.label(if changed { text.color(theme::accent()).strong() } else { text }).on_hover_text(crate::i18n::tf!("${at:0digits$X} = {byte}"));
                                }
                                None => {
                                    ui.label(mono("--".into()).color(theme::text_faint())).on_hover_text(crate::i18n::tf!("${at:0digits$X} hasn't been read"));
                                }
                            }
                        }
                        let chars: String = (0..16u32).map(|i| peek(address.wrapping_add(i) & mask)).map(|b| b.filter(|b| (32..127).contains(b)).map_or('.', char::from)).collect();
                        ui.label(mono(chars).color(theme::text_dim()));
                    });
                }
            }
            Err(problem) => {
                ui.label(RichText::new(problem).color(theme::problem_mark()).size(11.5));
            }
        }
    });
    shown_at
}

/// The breakpoints, each with the condition it stops on, and the addresses whose writing stops it: shown whether
/// it's running or stopped, so they can be set up first. `lines` gives a line's text.
pub fn breakpoints(ui: &mut Ui, code: &mut CodeMarks, lines: &dyn Fn(usize) -> String, conditions: bool, writes_supported: bool) {
    egui::CollapsingHeader::new(format!("Breakpoints ({})", code.breakpoints.len())).id_salt("debug-breakpoints").default_open(false).show(ui, |ui| {
        if code.breakpoints.is_empty() {
            ui.label(RichText::new(crate::i18n::tr("Click beside a line's number for a breakpoint.")).color(theme::text_faint()).size(11.5));
        }
        let lines_with: Vec<usize> = code.breakpoints.iter().copied().collect();
        for line in lines_with {
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tf!("{:>4}: {}", line + 1, lines(line).trim())).monospace().size(11.5));
            });
            if !conditions {
                continue;
            }
            let condition = code.conditions.entry(line).or_default();
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.label(RichText::new(crate::i18n::tr("only when")).size(11.5).color(theme::text_dim()));
                ui.add(egui::TextEdit::singleline(condition).hint_text(crate::i18n::tr("always, or e.g. A=0 && [count]>3")).desired_width(220.0));
            });
        }
        code.conditions.retain(|line, text| code.breakpoints.contains(line) && !text.trim().is_empty());
        ui.add_space(4.0);
        if !writes_supported {
            return;
        }
        ui.label(RichText::new(crate::i18n::tr("Stop when written to")).size(12.0));
        let mut gone = None;
        for (index, watch) in code.write_watches.iter().enumerate() {
            ui.horizontal(|ui| {
                if crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Stop watching this").clicked() {
                    gone = Some(index);
                }
                ui.label(RichText::new(watch).monospace().size(11.5));
            });
        }
        if let Some(index) = gone {
            code.write_watches.remove(index);
        }
        ui.horizontal(|ui| {
            let field = ui.add(egui::TextEdit::singleline(&mut code.new_write_watch).hint_text(crate::i18n::tr("An address or a label")).desired_width(160.0));
            if (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.small_button(crate::i18n::tr("Add")).clicked() {
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

/// Where a 68000 program is, as words: the address, and its label and line if it's in the program. `source` is the
/// address in the assembly.
pub fn place_68k(address: u32, source: Option<i64>, assembly: Option<&Assembly>) -> String {
    let in_program = source.zip(assembly).map(|(source, assembly)| {
        let line = assembly.line_at(source).map(|(file, line)| {
            let name = assembly.files.get(file).and_then(|f| f.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            format!(", line {} of {name}", line + 1)
        });
        let label = label_at(source, assembly).map(|l| format!(" {l}")).unwrap_or_default();
        format!("{label}{}", line.unwrap_or_default())
    });
    match in_program {
        Some(words) => format!("${address:08X}{words}"),
        None if (0xF8_0000..0x100_0000).contains(&(address & 0xFF_FFFF)) => crate::i18n::tf!("${address:08X}, in the Kickstart ROM"),
        None => crate::i18n::tf!("${address:08X}, outside the program"),
    }
}

/// The panel while a 68000 program on an Amiga is stopped: where, the buttons, the registers (each can be clicked
/// to change it), the calls it's inside, watches, a window on memory, the custom chips' registers and what's been
/// printed. `cursor` is the address of the line the cursor is on, for Run to cursor. Gives what's to be asked of
/// the debugger, memory to read among it.
pub fn stopped_68k(ui: &mut Ui, stop: &Stop, shown: &mut crate::amiga_debug::Shown, assembly: Option<&Assembly>, code: &mut CodeMarks, cursor: Option<i64>) -> Vec<crate::remote_debug::Ask> {
    use crate::amiga_debug::{self as amiga, AmigaAsk};
    use crate::remote_debug::Ask;
    let mut asks = Vec::new();
    let Some(now) = stop.amiga.as_deref() else {
        return asks;
    };
    let mono = |text: String| RichText::new(text).monospace().size(12.0);
    let r = now.registers;
    let before = shown.before.clone().filter(|b| b.serial != now.serial);
    let labels = assembly.map(|a| amiga::relocate_symbols(&a.symbols, &shown.program, &now.bases));
    // Memory an expression wanted that hasn't been read, to be read for next time.
    let missing = std::cell::RefCell::new(Vec::<u32>::new());
    let peek = |a: u32| {
        now.peek(a).unwrap_or_else(|| {
            missing.borrow_mut().push(a);
            0
        })
    };
    let names = |name: &str| {
        amiga::register_value(name, &r).or_else(|| {
            // The custom chips' registers by name, unless the program has a label of that name.
            let register = crate::amiga_hw::named(name).filter(|reg| !reg.is_cia() && !labels.as_ref().is_some_and(|l| l.contains_key(name)))?;
            Some(i64::from(u16::from_be_bytes([peek(register.address), peek(register.address + 1)])))
        })
    };
    let wide = Wide { names: &names, peek: &peek };
    ui.add_space(4.0);
    egui::Frame::new().fill(ui.visuals().faint_bg_color).inner_margin(egui::Margin::same(6)).corner_radius(4.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::i18n::tf!("Stopped at {}", place_68k(now.pc(), now.source, assembly))).strong());
        });
        ui.horizontal(|ui| {
            if ui.small_button(crate::i18n::tr("Carry on")).on_hover_text(crate::i18n::tr("Runs on until the next breakpoint")).clicked() {
                asks.push(Ask::CarryOn);
            }
            if ui.small_button(crate::i18n::tr("Step")).on_hover_text(crate::i18n::tr("Runs to the next line, into a subroutine of the program's (a call into the ROM or a library is run whole)")).clicked() {
                asks.push(Ask::Step);
            }
            if ui.small_button(crate::i18n::tr("Step over")).on_hover_text(crate::i18n::tr("Runs to the next line, running any subroutine it calls whole")).clicked() {
                asks.push(Ask::StepOver);
            }
            let to_cursor = ui.add_enabled(cursor.is_some(), egui::Button::new(crate::i18n::tr("Run to cursor")).small()).on_hover_text(crate::i18n::tr("Runs on until the line the cursor is on, or a breakpoint before it")).on_disabled_hover_text(crate::i18n::tr("Put the cursor on a line that makes code"));
            if to_cursor.clicked()
                && let Some(address) = cursor
            {
                asks.push(Ask::Amiga(AmigaAsk::RunTo(address)));
            }
        });
        if let Some(reason) = &stop.reason {
            ui.label(RichText::new(reason).color(theme::accent()).size(12.0));
        }
        // The registers: click one to change it.
        let mut cell = |ui: &mut Ui, index: usize, asks: &mut Vec<Ask>| {
            let name = amiga::REGISTER_NAMES[index];
            let value = r[index];
            let width = if index == amiga::SR { 4 } else { 8 };
            if let Some((_, text)) = shown.editing.as_mut().filter(|(i, _)| *i == index) {
                let mut done = false;
                ui.horizontal(|ui| {
                    ui.label(mono(name.to_string()));
                    let field = ui.add(egui::TextEdit::singleline(text).font(egui::TextStyle::Monospace).desired_width(74.0));
                    if field.lost_focus() {
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let typed = text.trim().to_string();
                            let parsed = u32::from_str_radix(typed.trim_start_matches('$'), 16).map(i64::from).map_err(|e| e.to_string()).or_else(|_| eval_wide(&typed, &wide, labels.as_ref()));
                            if let Ok(value) = parsed {
                                asks.push(Ask::Amiga(AmigaAsk::SetRegister(index, value as u32)));
                            }
                        }
                        done = true;
                    } else if !field.has_focus() {
                        field.request_focus();
                    }
                });
                if done {
                    shown.editing = None;
                }
                return;
            }
            let changed = before.as_ref().is_some_and(|b| b.registers[index] != value);
            let text = mono(format!("{name} {value:0width$X}"));
            let label = ui.add(egui::Label::new(if changed { text.color(theme::accent()).strong() } else { text }).sense(egui::Sense::click()));
            if crate::access::called(label, &crate::i18n::tf!("{name}: click to change it")).on_hover_text(crate::i18n::tf!("{name} = {value} (${value:X}). Click to change it.")).clicked() {
                shown.editing = Some((index, format!("{value:0width$X}")));
            }
        };
        egui::Grid::new("m68k-registers").num_columns(4).spacing([14.0, 2.0]).show(ui, |ui| {
            for row in 0..4 {
                for column in 0..4 {
                    cell(ui, row * 4 + column, &mut asks);
                }
                ui.end_row();
            }
            cell(ui, amiga::PC, &mut asks);
            cell(ui, amiga::SR, &mut asks);
            ui.label(mono(amiga::status_words(r[amiga::SR])));
            ui.label(mono(now.long(r[amiga::A7]).map(|top| format!("(SP) {top:08X}")).unwrap_or_default()));
            ui.end_row();
        });
        // The calls it's inside.
        let calls = amiga::call_stack(now, &shown.program);
        egui::CollapsingHeader::new(crate::i18n::tf!("Call stack ({})", calls.len())).id_salt("debug-calls").default_open(true).show(ui, |ui| {
            ui.label(mono(format!("now   {}", place_68k(now.pc(), now.source, assembly))));
            for call in &calls {
                let at = shown.program.to_runtime(&now.bases, *call).unwrap_or(0);
                ui.label(mono(format!("from  {}", place_68k(at, Some(*call), assembly))));
            }
            if calls.is_empty() {
                ui.label(RichText::new(crate::i18n::tr("Not inside a JSR or BSR that can be seen.")).color(theme::text_faint()).size(11.5));
            }
        });
        watches_section(ui, code, &|watch| eval_wide(watch, &wide, labels.as_ref()), "D0, A0.w, [count], long[ptr], COLOR00...");
        let at = memory_section(ui, &mut code.memory_at, "A0, $DFF180, a label...", &|text| if text.trim().is_empty() { Ok(i64::from(r[8])) } else { eval_wide(text, &wide, labels.as_ref()) }, 8, &|a| now.peek(a), &|a| before.as_ref().and_then(|b| b.peek(a)));
        if let Some(at) = at.filter(|at| !now.has(*at, 128)) {
            missing.borrow_mut().push(at);
            missing.borrow_mut().push(at.wrapping_add(127));
        }
        // Changing memory: bytes in hex, at an address or anything an expression gives.
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::i18n::tr("Change")).size(12.0));
            ui.add(egui::TextEdit::singleline(&mut shown.poke_at).hint_text(crate::i18n::tr("at")).desired_width(90.0));
            ui.label(RichText::new(crate::i18n::tr("to")).size(12.0));
            let field = ui.add(egui::TextEdit::singleline(&mut shown.poke_bytes).hint_text(crate::i18n::tr("bytes, as 4E71 or 12 34")).desired_width(130.0));
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if entered || ui.small_button(crate::i18n::tr("Write")).on_hover_text(crate::i18n::tr("Writes these bytes into the Amiga's memory")).clicked() {
                let digits: String = shown.poke_bytes.chars().filter(|c| !c.is_whitespace()).collect();
                let bytes: Option<Vec<u8>> = (digits.len().is_multiple_of(2) && !digits.is_empty()).then(|| digits.as_bytes().chunks(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap_or("x"), 16).ok()).collect()).flatten();
                if let (Ok(at), Some(bytes)) = (eval_wide(&shown.poke_at, &wide, labels.as_ref()), bytes) {
                    asks.push(Ask::Amiga(AmigaAsk::Write(at as u32, bytes)));
                }
            }
        });
        // The custom chips' registers, as the emulator keeps them: those not 0.
        egui::CollapsingHeader::new(crate::i18n::tr("Custom chips")).id_salt("debug-custom").default_open(false).show(ui, |ui| {
            if !now.has(amiga::CUSTOM, 2) {
                ui.label(RichText::new(crate::i18n::tr("The emulator didn't give them.")).color(theme::text_faint()).size(11.5));
                return;
            }
            egui::Grid::new("m68k-custom").num_columns(3).spacing([10.0, 1.0]).striped(true).show(ui, |ui| {
                for offset in (0..amiga::CUSTOM_SIZE).step_by(2) {
                    let (Some(register), Some(value)) = (crate::amiga_hw::custom(offset), now.word(amiga::CUSTOM + offset)) else { continue };
                    if value == 0 || register.offset() != offset {
                        continue;
                    }
                    let changed = before.as_ref().and_then(|b| b.word(amiga::CUSTOM + offset)).is_some_and(|b| b != value);
                    let name = mono(register.name.clone());
                    ui.label(if changed { name.color(theme::accent()).strong() } else { name }).on_hover_text(crate::amiga_hw::describe(register));
                    ui.horizontal(|ui| {
                        if register.colour().is_some() {
                            let (red, green, blue) = crate::amiga_hw::rgb12(value);
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                            ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(red * 17, green * 17, blue * 17));
                        }
                        ui.label(mono(format!("${value:04X}")));
                    });
                    let explained = crate::amiga_hw::explain(register, value);
                    let short: String = explained.chars().take(60).collect();
                    ui.label(RichText::new(short).size(11.5).color(theme::text_dim())).on_hover_text(crate::amiga_hw::field_lines(register, value).join("\n"));
                    ui.end_row();
                }
            });
        });
        if !now.output.trim().is_empty() {
            egui::CollapsingHeader::new(crate::i18n::tr("Output")).id_salt("debug-output").default_open(false).show(ui, |ui| {
                let lines: Vec<&str> = now.output.lines().collect();
                let recent = lines[lines.len().saturating_sub(40)..].join("\n");
                ui.label(RichText::new(recent).monospace().size(11.0));
            });
        }
    });
    // What expressions wanted that wasn't read: read it, a little around each, once.
    let mut regions: Vec<(u32, u32)> = missing.into_inner().into_iter().map(|a| (a & !0x7F, 0x80)).collect();
    regions.sort_unstable();
    regions.dedup();
    regions.retain(|region| !shown.reads.contains(region));
    if !regions.is_empty() {
        shown.reads.extend(regions.iter().copied());
        asks.push(Ask::Amiga(AmigaAsk::Read(regions)));
    }
    asks
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
        let assembly = crate::asm::assemble(&crate::asm::z80::Z80, std::path::Path::new("t.asm"), "  org $8000\nmain: nop\n  nop\n.loop: nop\nnext: ret\n", &|_| Err(crate::i18n::tr("no").into()));
        assert_eq!(label_of(0x8000, &assembly).as_deref(), Some("main"));
        assert_eq!(label_of(0x8002, &assembly).as_deref(), Some("main+2"), "local labels aren't used for this");
        assert_eq!(label_of(0x8003, &assembly).as_deref(), Some("next"));
        assert_eq!(label_of(0x7000, &assembly), None);
    }
}
