//! What 68000 source for the Atari ST does with the hardware and the system, for hover: which register a line
//! writes and what the value written means (a colour as the ST and STE show it, a timer's mode, a YM register
//! chosen a line before), which address registers point at the hardware, and which TOS function a trap calls,
//! worked out from the function number pushed just before it.
//!
//! It's read from the text, with the values of labels and EQUs from Limoniter's own assembler when it has them.

use crate::amiga_source::{evaluate, split, text_of};
use crate::asm::Assembly;
use crate::st_hw;
use crate::st_tos::{self, Trap};
use std::collections::HashMap;

/// A line of the source, split.
#[derive(Clone, Debug, Default)]
struct Line {
    mnemonic: String,
    operands: Vec<String>,
}

/// What a source does with the ST.
#[derive(Clone, Debug, Default)]
pub struct Source {
    /// Whether it looks like ST code at all, so that the ST's names aren't explained in someone else's.
    pub atari: bool,
    lines: Vec<Line>,
    /// Each time an address register is loaded: the line, the register (0-7), and the address if it's known.
    loads: Vec<(usize, u8, Option<u32>)>,
    symbols: HashMap<String, i64>,
    lower_symbols: HashMap<String, i64>,
}

/// An operand that's a 68000 address register.
fn address_register(s: &str) -> Option<u8> {
    let s = s.trim().to_ascii_lowercase();
    match s.as_str() {
        "sp" => Some(7),
        _ => s.strip_prefix('a').and_then(|n| n.parse::<u8>().ok()).filter(|n| *n < 8),
    }
}

/// An operand without a trailing size (.w, .l or .b), and whether it was .w (a short address, sign-extended).
fn strip_size(s: &str) -> (&str, bool) {
    let lower = s.to_ascii_lowercase();
    if lower.ends_with(".w") {
        (&s[..s.len() - 2], true)
    } else if lower.ends_with(".l") || lower.ends_with(".b") {
        (&s[..s.len() - 2], false)
    } else {
        (s, false)
    }
}

/// Whether an operand pushes onto the stack: -(sp) or -(a7).
fn is_push_target(operand: &str) -> bool {
    let lower = operand.trim().to_ascii_lowercase();
    lower == "-(sp)" || lower == "-(a7)"
}

/// The size an instruction works on, from its suffix: 1, 2 or 4 bytes (a word if it has none).
fn size_of(mnemonic: &str) -> u32 {
    match mnemonic.rsplit_once('.').map(|(_, s)| s) {
        Some("b") => 1,
        Some("l") => 4,
        _ => 2,
    }
}

impl Source {
    /// Reads a document's text, with what the assembler made of it if there is that.
    pub fn scan(text: &str, assembly: Option<&Assembly>) -> Source {
        let mut source = Source::default();
        if let Some(assembly) = assembly {
            source.symbols = assembly.symbols.iter().map(|(k, v)| (k.clone(), *v)).collect();
            source.lower_symbols = assembly.symbols.iter().map(|(k, v)| (k.to_ascii_lowercase(), *v)).collect();
        }
        let lower = text.to_ascii_lowercase();
        // Amiga code is the Amiga's: its hover explains it.
        let amiga = ["$dff", "_lvo", "execbase", "_custom", "hardware/custom.i", "4.w,a6"].iter().any(|h| lower.contains(h));
        let mut atari = false;
        for (n, text_line) in text.lines().enumerate() {
            let parts = split(text_line);
            let line = Line {
                mnemonic: parts.mnemonic.map(|m| text_of(text_line, m).to_ascii_lowercase()).unwrap_or_default(),
                operands: parts.operands.iter().map(|s| text_of(text_line, *s)).collect(),
            };
            if let Some((register, value)) = source.load(&line) {
                source.loads.push((n, register, value));
            }
            source.lines.push(line);
        }
        for (n, line) in source.lines.iter().enumerate() {
            if atari {
                break;
            }
            if line.mnemonic == "trap" && line.operands.first().and_then(|o| o.strip_prefix('#')).and_then(|e| source.eval(e)).is_some_and(|t| matches!(t, 1 | 13 | 14)) {
                atari = true;
            }
            atari |= line.operands.iter().any(|o| source.target(o, n).is_some());
        }
        source.atari = atari && !amiga;
        source
    }

    fn lookup(&self, name: &str) -> Option<i64> {
        self.symbols.get(name).or_else(|| self.lower_symbols.get(&name.to_ascii_lowercase())).copied()
    }

    fn eval(&self, expr: &str) -> Option<i64> {
        evaluate(expr.trim(), &|name| self.lookup(name))
    }

    fn immediate(&self, operand: &str) -> Option<i64> {
        operand.trim().strip_prefix('#').and_then(|e| self.eval(e))
    }

    /// An address register being loaded on a line: which, and with what if it's known.
    fn load(&self, line: &Line) -> Option<(u8, Option<u32>)> {
        let [from, to] = &line.operands[..] else {
            return None;
        };
        let register = address_register(to)?;
        let value = match line.mnemonic.as_str() {
            "lea" | "lea.l" => {
                let lower = from.to_ascii_lowercase();
                let expr = lower.strip_suffix("(pc)").map(|e| &from[..e.len()]).unwrap_or(from);
                self.address(expr)
            }
            "move.l" | "movea.l" | "movea" => self.immediate(from).map(|v| v as u32 & 0xFF_FFFF),
            _ => return None,
        };
        Some((register, value))
    }

    /// An absolute address written as an operand: $FF8240, $FFFF8240.W, or $8240.W (sign-extended).
    fn address(&self, operand: &str) -> Option<u32> {
        let (expr, short) = strip_size(operand.trim());
        let mut value = self.eval(expr)?;
        if short && (0x8000..=0xFFFF).contains(&value) {
            value |= !0xFFFF;
        }
        Some(value as u32 & 0xFF_FFFF)
    }

    /// What an address register holds on a line, as far as the loads above it say.
    fn base_at(&self, line: usize, register: u8) -> Option<u32> {
        self.loads.iter().rfind(|(l, r, _)| *l < line && *r == register).and_then(|(_, _, v)| *v)
    }

    /// The ST hardware address an operand reaches on a line.
    fn target(&self, operand: &str, line: usize) -> Option<u32> {
        let operand = operand.trim();
        if operand.starts_with('#') || operand.starts_with('-') && operand.ends_with(')') || operand.ends_with(")+") {
            return None;
        }
        if let Some(open) = operand.find('(')
            && operand.ends_with(')')
        {
            let register = address_register(&operand[open + 1..operand.len() - 1])?;
            let expr = operand[..open].trim();
            let offset = if expr.is_empty() { 0 } else { self.eval(expr)? };
            let address = self.base_at(line, register)?.wrapping_add(offset as u32) & 0xFF_FFFF;
            return st_hw::is_hardware(address).then_some(address);
        }
        let address = self.address(operand)?;
        (st_hw::is_hardware(address) && st_hw::at(address).is_some()).then_some(address)
    }

    /// What the pointer resting on a line is over, said: a TOS call, a register being written (and the value
    /// written, read into its bits), a bit being tested, or a register's or function's name.
    pub fn describe(&self, line: usize, text: &str, col: usize) -> Option<String> {
        if !self.atari {
            return None;
        }
        let parts = split(text);
        let operand = parts.operands.iter().position(|(s, e)| *s <= col && col <= *e);
        let on_mnemonic = parts.mnemonic.is_some_and(|(s, e)| s <= col && col <= e);
        let here = Line { mnemonic: parts.mnemonic.map(|m| text_of(text, m).to_ascii_lowercase()).unwrap_or_default(), operands: parts.operands.iter().map(|s| text_of(text, *s)).collect() };
        if here.mnemonic == "trap" && (on_mnemonic || operand.is_some()) {
            return self.describe_trap(line, &here);
        }
        if let Some(index) = operand {
            // Line-A, as dc.w $A00A.
            if here.mnemonic == "dc.w"
                && let Some(said) = self.eval(&here.operands[index]).and_then(|v| st_tos::line_a(v as u16))
            {
                return Some(said);
            }
            // The function number pushed for the trap that follows.
            if let Some(said) = self.describe_push(line, &here) {
                return Some(said);
            }
            if let Some(said) = self.describe_instruction(line, &here, index) {
                return Some(said);
            }
        }
        // A name anywhere else, such as where it's defined: a register's, or a TOS function's.
        let word = word_at(text, col)?;
        let value = self.lookup(&word);
        if let Some(v) = value
            && let address = v as u32 & 0xFF_FFFF
            && st_hw::is_hardware(address)
            && let Some(found) = st_hw::at(address)
        {
            return Some(st_hw::describe(found));
        }
        if let Some(function) = st_tos::named(&word).filter(|f| value.is_none_or(|v| v == f.number as i64)) {
            return Some(function.describe());
        }
        st_hw::named(&word).filter(|_| value.is_none()).map(st_hw::describe)
    }

    /// The lines that put things on the stack for a trap, nearest first, from the line before `line` back to the
    /// last trap or jump: each line's number, size in bytes and what was pushed.
    fn pushes_before(&self, line: usize) -> Vec<(usize, u32, String)> {
        let mut found = Vec::new();
        for n in (line.saturating_sub(40)..line).rev() {
            let l = &self.lines[n];
            let m = l.mnemonic.as_str();
            if m.is_empty() {
                continue;
            }
            let base = m.split('.').next().unwrap_or(m);
            if matches!(base, "trap" | "rts" | "rte" | "bra" | "jmp" | "jsr" | "bsr" | "dbra" | "dbf") || l.operands.last().is_some_and(|o| o.eq_ignore_ascii_case("sp") || o.eq_ignore_ascii_case("a7")) {
                break;
            }
            match (base, &l.operands[..]) {
                ("pea", [what]) => found.push((n, 4, what.clone())),
                ("move" | "clr", ops) if ops.last().is_some_and(|o| is_push_target(o)) => {
                    let what = if base == "clr" { "#0".to_string() } else { ops[0].clone() };
                    found.push((n, size_of(m), what));
                }
                _ => {}
            }
        }
        found
    }

    /// The trap a function-number push is for: the next line with an instruction, if it's a trap.
    fn trap_after(&self, line: usize) -> Option<(usize, i64)> {
        let next = (line + 1..self.lines.len().min(line + 6)).find(|n| !self.lines[*n].mnemonic.is_empty())?;
        let l = &self.lines[next];
        (l.mnemonic == "trap").then(|| l.operands.first().and_then(|o| self.immediate(o)).map(|t| (next, t))).flatten()
    }

    fn describe_push(&self, line: usize, here: &Line) -> Option<String> {
        if !(here.mnemonic == "move.w" && here.operands.len() == 2 && is_push_target(&here.operands[1])) {
            return None;
        }
        let number = self.immediate(&here.operands[0])?;
        let (_, trap) = self.trap_after(line)?;
        let trap = Trap::from_number(trap)?;
        let function = st_tos::function(trap, number)?;
        Some(crate::i18n::tf!("The function number for the trap that follows:\n{}", function.describe()))
    }

    fn describe_trap(&self, line: usize, here: &Line) -> Option<String> {
        let n = here.operands.first().and_then(|o| self.immediate(o))?;
        if n == 2 {
            // AES or VDI: the value in d0.
            let d0 = (line.saturating_sub(20)..line).rev().map(|i| &self.lines[i]).take_while(|l| l.mnemonic != "trap").find_map(|l| {
                let [from, to] = &l.operands[..] else {
                    return None;
                };
                (to.eq_ignore_ascii_case("d0") && matches!(l.mnemonic.as_str(), "move.w" | "move.l" | "moveq" | "move")).then(|| self.immediate(from))
            });
            return Some(st_tos::trap2(d0.flatten()));
        }
        let Some(trap) = Trap::from_number(n) else {
            return Some(crate::i18n::tf!("trap #{n}: not one TOS uses for its calls (1 GEMDOS, 2 AES and VDI, 13 BIOS, 14 XBIOS)"));
        };
        let pushes = self.pushes_before(line);
        let Some((first_line, size, what)) = pushes.first() else {
            return Some(crate::i18n::tf!("{}\nNo function number is pushed just before it here.", trap.about()));
        };
        let number = (*size == 2).then(|| self.immediate(what)).flatten();
        let Some(number) = number else {
            return Some(crate::i18n::tf!("{}\nThe function's number comes from line {} ({what}), which isn't known until it runs.", trap.about(), first_line + 1));
        };
        let Some(function) = st_tos::function(trap, number) else {
            return Some(crate::i18n::tf!("{} function {number} (${number:02X}): not one Limoniter knows", trap.name()));
        };
        let mut out = function.describe();
        // What the lines above push for each argument, the nearest first.
        let mut lines = Vec::new();
        for (arg, (n, size, what)) in function.args.iter().zip(pushes.iter().skip(1)) {
            let want = if arg.long { 4 } else { 2 };
            let warning = if *size != want { crate::i18n::tf!("  (a {}, but it should be a {})", if *size == 4 { "long" } else { "word" }, if want == 4 { "long" } else { "word" }) } else { String::new() };
            lines.push(format!("  {}: {what} (line {}){warning}", arg.name, n + 1));
        }
        if !lines.is_empty() {
            out.push_str("\nHere:\n");
            out.push_str(&lines.join("\n"));
        }
        if pushes.len() - 1 < function.args.len() && !function.args.is_empty() {
            out.push_str(&crate::i18n::tf!("\n{} of its {} arguments are pushed in the lines just above.", pushes.len() - 1, function.args.len()));
        }
        Some(out)
    }

    /// The YM register chosen for a write to $FF8802 on a line: from the latest write to $FF8800 above it.
    fn ym_selected(&self, line: usize) -> Option<u8> {
        for n in (line.saturating_sub(30)..line).rev() {
            let l = &self.lines[n];
            if l.operands.len() != 2 || !l.mnemonic.starts_with("move") {
                continue;
            }
            let Some(address) = self.target(&l.operands[1], n) else {
                continue;
            };
            let value = self.immediate(&l.operands[0]);
            match (address, size_of(&l.mnemonic)) {
                (0xFF8800, 1 | 2) => return value.map(|v| (if size_of(&l.mnemonic) == 2 { v >> 8 } else { v }) as u8 & 15),
                (0xFF8800, 4) => return value.map(|v| (v >> 24) as u8 & 15),
                _ => {}
            }
        }
        None
    }

    fn describe_instruction(&self, line: usize, here: &Line, index: usize) -> Option<String> {
        let mnemonic = here.mnemonic.as_str();
        let operands = &here.operands;
        let base = mnemonic.split('.').next().unwrap_or(mnemonic);
        let targets: Vec<Option<u32>> = operands.iter().map(|o| self.target(o, line)).collect();
        // A bit tested or changed: on memory, the 68000 works on a byte, so a word register's bit at its even
        // address is in its high byte.
        if matches!(base, "btst" | "bset" | "bclr" | "bchg")
            && operands.len() == 2
            && let Some(n) = self.immediate(&operands[0])
            && let Some(address) = targets[1]
        {
            let register = st_hw::at(address)?;
            let n = (n & 7) as u8;
            let offset = address - register.address - register.index(address) * register.size.bytes();
            let shift = (register.size.bytes() - 1 - offset) * 8;
            return Some(st_hw::describe_bit(register, n + shift as u8));
        }
        // Which operand is the register: the one under the pointer, or, over an immediate value, the other.
        let at = match targets.get(index).copied().flatten() {
            Some(_) => index,
            None if operands[index].starts_with('#') => targets.iter().rposition(Option::is_some)?,
            None => return None,
        };
        let address = targets[at]?;
        let size = size_of(mnemonic);
        let value = if base == "clr" { Some(0) } else { operands.iter().enumerate().filter(|(i, _)| *i != at).find_map(|(_, o)| self.immediate(o)) };
        let writes = at == operands.len() - 1 && operands.len() > 1 || base == "clr";
        if base == "movem" {
            let register = st_hw::at(address)?;
            return Some(crate::i18n::tf!("{}\nMOVEM moves a run of registers from here on: {} bytes", st_hw::describe(register), "a long each (.l) or a word each (.w),"));
        }
        // The bytes written, and each register they reach.
        let mut said = Vec::new();
        let mut seen: Vec<(u32, u32)> = Vec::new();
        for a in address..address + size {
            let Some(register) = st_hw::at(a) else {
                continue;
            };
            let element = register.address + register.index(a) * register.size.bytes();
            if seen.contains(&(register.address, element)) {
                continue;
            }
            seen.push((register.address, element));
            let Some(v) = value.filter(|_| writes) else {
                said.push(st_hw::describe(register));
                continue;
            };
            // The register's bytes from the value, as far as the write reaches them.
            let bytes = (v as u64).to_be_bytes();
            let mut reg_value = 0u32;
            let mut partial = false;
            for b in 0..register.size.bytes() {
                let place = element + b;
                reg_value <<= 8;
                if (address..address + size).contains(&place) {
                    reg_value |= bytes[(8 - size + place - address) as usize] as u32;
                } else {
                    partial = true;
                }
            }
            if register.address == 0xFF8802 {
                let byte = reg_value as u8;
                let chosen = if size == 4 && address == 0xFF8800 { Some((v >> 24) as u8 & 15) } else { self.ym_selected(line) };
                said.push(match chosen {
                    Some(r) => format!("{}\nYM register {r} ({}) = ${byte:02X}: {}", register.heading(), st_hw::YM[r as usize].name, st_hw::explain_ym(r, byte)),
                    None => crate::i18n::tf!("{}\n${byte:02X} to the YM register chosen at $FF8800 (which isn't known here)", register.heading()),
                });
                continue;
            }
            let mut text = st_hw::describe_value(register, element, reg_value);
            if register.address == 0xFF8800 {
                let r = (reg_value & 15) as usize;
                text.push_str(&format!("\nRegister {r}: {}", st_hw::YM[r].about));
            }
            if partial {
                text.push_str("\n(only part of the register is written here)");
            }
            said.push(text);
        }
        (!said.is_empty()).then(|| said.join("\n\n"))
    }

    /// The names the source could use for a TOS function in a comment, starting with what's typed: the ones on
    /// this line's push or trap first.
    pub fn function_names(prefix: &str) -> Vec<String> {
        let lower = prefix.to_ascii_lowercase();
        st_tos::FUNCTIONS.iter().filter(|f| f.name.to_ascii_lowercase().starts_with(&lower) && f.name != prefix).map(|f| f.name.to_string()).collect()
    }
}

/// The name under a column: letters, digits and underscores.
fn word_at(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let at = col.min(chars.len());
    let mut start = at;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = at;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    (start < end && !chars[start].is_ascii_digit() && chars.get(start.wrapping_sub(1)) != Some(&'$')).then(|| chars[start..end].iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO: &str = "start:\tclr.l\t-(sp)\n\
\tmove.w\t#$20,-(sp)\t; Super\n\
\ttrap\t#1\n\
\taddq.l\t#6,sp\n\
\tmove.w\t#$777,$ff8240.w\n\
\tmove.b\t#8,$fffffa1b.w\n\
\tmove.b\t#7,$ff8800.w\n\
\tmove.b\t#$3f,$ff8802.w\n\
\tbset\t#0,$fffa07.w\n\
\tlea\t$ffff8a00.w,a0\n\
\tmove.w\t#$0203,$3a(a0)\n\
\tpea\tmessage\n\
\tmove.w\t#9,-(sp)\n\
\ttrap\t#1\n\
\taddq.l\t#6,sp\n\
\tmove.l\t#$08001000,$ffff8800.w\n\
\tdc.w\t$a00a\n\
\tmove.w\t#$8240,d0\n\
\tclr.b\t$fffa1b.w\n\
\tmove.w\t#$700,$8242.w\n\
message:\tdc.b\t\"hi\",0\n";

    fn hover(source: &Source, line: usize, find: &str) -> String {
        let text = DEMO.lines().nth(line).unwrap();
        let col = text.find(find).unwrap_or_else(|| panic!("{find} in {text}")) + 1;
        source.describe(line, text, col).unwrap_or_else(|| panic!("nothing over {find} in {text}"))
    }

    #[test]
    fn st_source_is_told_from_the_amiga_s() {
        let source = Source::scan(DEMO, None);
        assert!(source.atari);
        assert!(!Source::scan("\tlea\t$dff000,a6\n\tmove.w\t#$0fff,$180(a6)\n\tclr.w\t-(sp)\n\ttrap\t#1\n", None).atari, "the Amiga's chips: Amiga code");
        assert!(!Source::scan("\tmove.w\t#1,d0\n\trts\n", None).atari);
        assert!(Source::scan("\tclr.w\t-(sp)\n\ttrap\t#1\n", None).atari);
    }

    #[test]
    fn traps_are_named_from_the_number_pushed() {
        let source = Source::scan(DEMO, None);
        let said = hover(&source, 2, "trap");
        assert!(said.starts_with("GEMDOS Super ($20, 32), trap #1"), "{said}");
        assert!(said.contains("stack: #0 (line 1)"), "Super's argument is the clr.l: {said}");
        let said = hover(&source, 13, "trap");
        assert!(said.starts_with("GEMDOS Cconws") && said.contains("string: message (line 12)"), "{said}");
        let said = hover(&source, 12, "#9");
        assert!(said.contains("GEMDOS Cconws"), "over the push, the function too: {said}");
        assert!(hover(&source, 16, "a00a").contains("Hide mouse"));
        let aes = Source::scan("\tmove.w\t#200,d0\n\tmove.l\t#params,d1\n\ttrap\t#2\n\tclr.w -(sp)\n\ttrap #1\n", None);
        assert!(aes.describe(2, "\ttrap\t#2", 2).unwrap().starts_with("AES"));
        let xbios = Source::scan("\tmove.w\t#-1,-(sp)\n\tmove.l\td0,-(sp)\n\tmove.l\td0,-(sp)\n\tmove.w\t#5,-(sp)\n\ttrap\t#14\n\tlea\t12(sp),sp\n", None);
        let said = xbios.describe(4, "\ttrap\t#14", 2).unwrap();
        assert!(said.starts_with("XBIOS Setscreen") && said.contains("logical: d0 (line 3)") && said.contains("resolution: #-1 (line 1)"), "{said}");
        let unknown = Source::scan("\tmove.w\td7,-(sp)\n\ttrap\t#14\n", None);
        assert!(unknown.describe(1, "\ttrap\t#14", 2).unwrap().contains("isn't known until it runs"));
    }

    #[test]
    fn writes_to_registers_are_explained() {
        let source = Source::scan(DEMO, None);
        let said = hover(&source, 4, "$777");
        assert!(said.starts_with("COLOR0") && said.contains("ST:  \u{25A0}#FFFFFF") && said.contains("STE: \u{25A0}#EEEEEE"), "{said}");
        assert!(hover(&source, 5, "fffffa1b").contains("Timer B: event count mode"));
        let said = hover(&source, 6, "ff8800");
        assert!(said.contains("chooses register 7: Mixer"), "{said}");
        let said = hover(&source, 7, "$3f");
        assert!(said.contains("YM register 7 (Mixer) = $3F: tone on none; noise on none"), "{said}");
        assert!(hover(&source, 8, "fffa07").contains("TIMERB"), "bit 0 of IERA");
        let said = hover(&source, 10, "$0203");
        assert!(said.contains("HOP 2: the source is the source") && said.contains("OP 3: D = S (copy)"), "{said}");
        let said = hover(&source, 15, "$08001000");
        assert!(said.contains("YM register 8 (Channel A volume) = $10: channel A follows the envelope"), "{said}");
        assert!(source.describe(17, DEMO.lines().nth(17).unwrap(), 10).is_none(), "$8240 into d0 is only a number");
        assert!(hover(&source, 18, "fffa1b").contains("Timer B: stopped"), "CLR writes 0");
        assert!(hover(&source, 19, "8242").contains("colour 1"), "$8242.w is $FFFF8242");
    }

    #[test]
    fn names_are_explained_with_the_assembler_s_values() {
        let text = "palette\tequ\t$ff8240\ntbcr\tequ\t$fffffa1b\nCconws\tequ\t9\nstart:\tmove.w\t#$700,palette+2\n\tmove.b\t#1,tbcr.w\n\tmove.w\t#Cconws,-(sp)\n\ttrap\t#1\n";
        let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::M68k, std::path::Path::new("t.s"), text).assembly;
        let source = Source::scan(text, Some(&assembly));
        assert!(source.atari);
        let line = |n: usize| text.lines().nth(n).unwrap();
        let said = source.describe(3, line(3), line(3).find("palette").unwrap() + 1).unwrap();
        assert!(said.starts_with("COLOR1") && said.contains("red 7, green 0, blue 0"), "{said}");
        let said = source.describe(4, line(4), line(4).find("tbcr").unwrap() + 1).unwrap();
        assert!(said.contains("delay mode, the 2.4576 MHz clock divided by 4"), "{said}");
        assert!(source.describe(1, line(1), 1).unwrap().starts_with("TBCR"));
        assert!(source.describe(6, line(6), 2).unwrap().starts_with("GEMDOS Cconws"));
        assert!(source.describe(2, line(2), 2).unwrap().starts_with("GEMDOS Cconws"), "the EQU's name");
    }

    #[test]
    fn hover_comes_through_code_intel_and_the_amiga_s_stays_away() {
        let text = DEMO;
        let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::M68k, std::path::Path::new("t.s"), text).assembly;
        let amiga = crate::amiga_source::Source::scan(text, Some(&assembly));
        let st = Source::scan(text, Some(&assembly));
        let intel = crate::code_intel::CodeIntel { hardware: Some(std::sync::Arc::new(amiga)), atari: Some(std::sync::Arc::new(st)), ..crate::code_intel::CodeIntel::for_cpu(crate::assemblers::Cpu::M68k) };
        let line = text.lines().nth(4).unwrap();
        assert!(intel.describe(4, line, line.find("$777").unwrap() + 1).unwrap().starts_with("COLOR0"));
        assert!(intel.describe(2, "\ttrap\t#1 ; Super", 15).is_none(), "nothing in a comment");
        assert!(intel.completions(1, "\tmove.w\t#$20,-(sp)\t; Sup", 24).contains(&"Super".to_string()), "names in a comment");
        // And Amiga code gets the Amiga's hover, not the ST's.
        let amiga_text = "\tlea\t$dff000,a6\n\tmove.w\t#$0f00,$180(a6)\n";
        let intel = crate::code_intel::CodeIntel {
            hardware: Some(std::sync::Arc::new(crate::amiga_source::Source::scan(amiga_text, None))),
            atari: Some(std::sync::Arc::new(Source::scan(amiga_text, None))),
            ..crate::code_intel::CodeIntel::for_cpu(crate::assemblers::Cpu::M68k)
        };
        assert!(intel.describe(1, "\tmove.w\t#$0f00,$180(a6)", 10).unwrap().starts_with("COLOR00"));
    }
}
