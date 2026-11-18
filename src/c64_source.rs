//! What 6502 source for the Commodore 64 does with the hardware, for hover: which register a line reads or writes,
//! the value written read into its bits (worked out from the LDA #, LDX # or LDY # before it), the KERNAL routine
//! a JSR calls, and the vector a store fills. Over an LDA #value, the register it's next stored to.
//!
//! It's read from the text, with the values of labels from Limoniter's own assembler when it has them.

use crate::asm::Assembly;
use crate::c64_hw::{self, Access, Chip, Register};
use std::collections::HashMap;

/// A line of the source, split.
#[derive(Clone, Debug, Default)]
struct Line {
    /// Whether a label is defined on it, so that code may come to it from elsewhere.
    label: bool,
    mnemonic: String,
    operand: String,
    /// Where the mnemonic and the operand are, by characters.
    mnemonic_at: (usize, usize),
    operand_at: (usize, usize),
}

/// What a source does with the C64.
#[derive(Clone, Debug, Default)]
pub struct Source {
    /// Whether it looks like C64 code at all, so another 6502 machine's isn't explained as the C64's.
    pub c64: bool,
    lines: Vec<Line>,
    symbols: HashMap<String, i64>,
    lower_symbols: HashMap<String, i64>,
}

const INSTRUCTIONS: [&str; 56] = [
    "adc", "and", "asl", "bcc", "bcs", "beq", "bit", "bmi", "bne", "bpl", "brk", "bvc", "bvs", "clc", "cld", "cli", "clv", "cmp", "cpx", "cpy", "dec", "dex", "dey", "eor", "inc", "inx", "iny", "jmp", "jsr", "lda", "ldx", "ldy", "lsr", "nop", "ora", "pha", "php", "pla", "plp", "rol", "ror", "rti", "rts", "sbc", "sec", "sed", "sei", "sta", "stx", "sty", "tax", "tay", "tsx", "txa", "txs", "tya",
];

fn is_instruction(word: &str) -> bool {
    INSTRUCTIONS.contains(&word)
}

/// The code of a line, without its comment (; or //, outside quotes).
fn code_of(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quote: Option<u8> = None;
    for i in 0..bytes.len() {
        match quote {
            Some(q) if bytes[i] == q => quote = None,
            Some(_) => {}
            None => match bytes[i] {
                b';' => return &line[..i],
                b'/' if bytes.get(i + 1) == Some(&b'/') => return &line[..i],
                b'"' => quote = Some(b'"'),
                _ => {}
            },
        }
    }
    line
}

/// A line split into its label, mnemonic and operand, by characters.
fn split(text: &str) -> Line {
    let code = code_of(text);
    let chars: Vec<char> = code.chars().collect();
    let mut i = 0;
    let mut line = Line::default();
    let word_end = |from: usize| (from..chars.len()).find(|j| chars[*j].is_whitespace()).unwrap_or(chars.len());
    let skip_space = |from: usize| (from..chars.len()).find(|j| !chars[*j].is_whitespace()).unwrap_or(chars.len());
    // A label: in the first column, or ending in a colon (!name: and - and + too).
    if !chars.is_empty() && !chars[0].is_whitespace() {
        let end = word_end(0);
        let word: String = chars[..end].iter().collect::<String>().to_ascii_lowercase();
        let word = word.trim_end_matches(':');
        if !is_instruction(word) {
            line.label = true;
            i = end;
        }
    }
    i = skip_space(i);
    let mut end = word_end(i);
    let mut word: String = chars[i..end].iter().collect();
    if word.ends_with(':') {
        line.label = true;
        i = skip_space(end);
        end = word_end(i);
        word = chars[i..end].iter().collect();
    }
    if word.is_empty() {
        return line;
    }
    line.mnemonic = word.to_ascii_lowercase();
    line.mnemonic_at = (i, end);
    let start = skip_space(end);
    let operand: String = chars[start..].iter().collect();
    let operand = operand.trim_end().to_string();
    line.operand_at = (start, start + operand.chars().count());
    line.operand = operand;
    line
}

/// An operand's address part: without #, an index (,x ,y), brackets, or a size suffix.
fn address_part(operand: &str) -> Option<&str> {
    let operand = operand.trim();
    if operand.starts_with('#') || operand.is_empty() {
        return None;
    }
    let lower = operand.to_ascii_lowercase();
    let mut body = operand;
    for suffix in [",x", ",y", "),y", ",x)"] {
        if lower.replace(' ', "").ends_with(suffix)
            && let Some(at) = operand.rfind(',')
        {
            body = &operand[..at];
            break;
        }
    }
    let body = body.trim();
    let body = body.strip_prefix('(').map(|b| b.trim_end_matches(')')).unwrap_or(body);
    Some(body.trim())
}

/// Which register an instruction loads (a, x or y), or changes some other way.
fn changes(mnemonic: &str, operand: &str) -> &'static [char] {
    let accumulator = operand.is_empty() || operand.eq_ignore_ascii_case("a");
    match mnemonic {
        "lda" | "pla" | "txa" | "tya" | "adc" | "sbc" | "and" | "ora" | "eor" => &['a'],
        "asl" | "lsr" | "rol" | "ror" if accumulator => &['a'],
        "ldx" | "tax" | "tsx" | "inx" | "dex" => &['x'],
        "ldy" | "tay" | "iny" | "dey" => &['y'],
        // A routine may change any of them.
        "jsr" => &['a', 'x', 'y'],
        _ => &[],
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
        source.lines = text.lines().map(split).collect();
        let lower = text.to_ascii_lowercase();
        // Another 6502 machine's: the Atari's, the NES's, the BBC's, the Apple II's.
        let elsewhere = ["wsync", "colbk", "colpf", "inesprg", "ppuctrl", "ppumask", "oswrch", "osasci", "osbyte", "$c030", "cout1"].iter().any(|w| lower.contains(w));
        let mut hits = 0;
        for line in &source.lines {
            if !is_instruction(&line.mnemonic) {
                if line.mnemonic.contains("basicupstart") {
                    hits += 1;
                }
                continue;
            }
            if let Some(address) = source.target(line) {
                let register = c64_hw::at(address);
                hits += match register {
                    Some(_) if c64_hw::is_chip(address) => 1,
                    Some(r) if r.access == Access::Call && line.mnemonic == "jsr" => 1,
                    _ => 0,
                };
            }
        }
        source.c64 = hits > 0 && !elsewhere || lower.contains("basicupstart");
        source
    }

    fn lookup(&self, name: &str) -> Option<i64> {
        self.symbols.get(name).or_else(|| self.lower_symbols.get(&name.to_ascii_lowercase())).copied()
    }

    /// An expression's value: $hex, %binary, decimal, 'c', labels, < and > for the low and high bytes.
    fn eval(&self, expr: &str) -> Option<i64> {
        let expr = expr.trim();
        if let Some(rest) = expr.strip_prefix('<') {
            return self.eval(rest).map(|v| v & 0xFF);
        }
        if let Some(rest) = expr.strip_prefix('>') {
            return self.eval(rest).map(|v| (v >> 8) & 0xFF);
        }
        if expr.len() == 3 && expr.starts_with('\'') && expr.ends_with('\'') {
            return expr.chars().nth(1).map(|c| c as i64);
        }
        crate::amiga_source::evaluate(expr, &|name| self.lookup(name.trim_start_matches('!')))
    }

    /// The address an instruction's operand reaches.
    fn target(&self, line: &Line) -> Option<u16> {
        let part = address_part(&line.operand)?;
        let value = self.eval(part)?;
        (0..=0xFFFF).contains(&value).then_some(value as u16)
    }

    /// The value a register (a, x or y) holds before a line, as far as the lines just above say: from an
    /// immediate load, with nothing changing it after, and no label (where code may come from elsewhere) between.
    fn held(&self, line: usize, register: char) -> Option<(u8, usize)> {
        for n in (line.saturating_sub(16)..line).rev() {
            let l = &self.lines[n];
            if !is_instruction(&l.mnemonic) {
                if l.label {
                    return None;
                }
                continue;
            }
            if changes(&l.mnemonic, &l.operand).contains(&register) {
                let load = format!("ld{register}");
                if l.mnemonic == load
                    && let Some(value) = l.operand.trim().strip_prefix('#').and_then(|e| self.eval(e))
                {
                    return Some((value as u8, n));
                }
                return None;
            }
            if l.label || matches!(l.mnemonic.as_str(), "rts" | "rti" | "jmp") {
                return None;
            }
        }
        None
    }

    /// The register a store writes from.
    fn stored(mnemonic: &str) -> Option<char> {
        match mnemonic {
            "sta" => Some('a'),
            "stx" => Some('x'),
            "sty" => Some('y'),
            _ => None,
        }
    }

    /// The value last stored to an address anywhere in the source, if it's known: for the VIC's bank when $D018
    /// is explained, say.
    fn last_stored(&self, address: u16, before: usize) -> Option<u8> {
        (0..before.min(self.lines.len())).rev().find_map(|n| {
            let l = &self.lines[n];
            let register = Self::stored(&l.mnemonic)?;
            (self.target(l).map(c64_hw::mirror) == Some(address)).then(|| self.held(n, register).map(|(v, _)| v)).flatten()
        })
    }

    /// What the pointer resting on a line is over, said: a register written (and what the value means), read,
    /// a KERNAL routine called, or a register's name.
    pub fn describe(&self, line: usize, text: &str, col: usize) -> Option<String> {
        if !self.c64 {
            return None;
        }
        let here = split(text);
        let over_operand = here.operand_at.0 <= col && col <= here.operand_at.1 && !here.operand.is_empty();
        let over_mnemonic = here.mnemonic_at.0 <= col && col <= here.mnemonic_at.1 && !here.mnemonic.is_empty();
        if is_instruction(&here.mnemonic) && (over_operand || over_mnemonic) {
            if let Some(said) = self.describe_instruction(line, &here) {
                return Some(said);
            }
            // Over LDA #value: where the value goes next.
            if over_operand && let Some(said) = self.describe_immediate(line, &here) {
                return Some(said);
            }
        }
        // A name: a label equated to a register, or a register's own name.
        let (_, _, word) = crate::code_intel::word_at(text, col)?;
        let word = word.trim_end_matches(':').trim_start_matches(['.', '!']).to_string();
        if let Some(value) = self.lookup(&word) {
            let found = (0x0300..=0xFFFF).contains(&value).then(|| c64_hw::at(value as u16)).flatten()?;
            if found.address == c64_hw::mirror(value as u16) || found.access == Access::Vector {
                return Some(format!("{word} = ${value:04X}\n{}", c64_hw::describe(found)));
            }
            return None;
        }
        c64_hw::named(&word).map(c64_hw::describe)
    }

    fn describe_instruction(&self, line: usize, here: &Line) -> Option<String> {
        let address = self.target(here)?;
        let part = address_part(&here.operand).unwrap_or("");
        let register = c64_hw::at(address)?;
        // $00 and $01 only as themselves (not $0001 or a label of value 1, which may be anything).
        if register.chip == Chip::CpuPort && !(part == "$01" || part == "$00" || part == "1" || part == "0" || part.eq_ignore_ascii_case("r6510") || part.eq_ignore_ascii_case("d6510")) {
            return None;
        }
        let mnemonic = here.mnemonic.as_str();
        if register.access == Access::Call {
            return matches!(mnemonic, "jsr" | "jmp").then(|| c64_hw::describe(register));
        }
        let named = if part.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') { format!("{part} = ${address:04X}: ") } else { String::new() };
        let mirror = if c64_hw::mirror(address) != address && c64_hw::is_chip(address) { format!("\n(a copy of ${:04X})", register.address) } else { String::new() };
        if register.access == Access::Vector {
            let described = c64_hw::describe(register);
            let mut out = if address == register.address { crate::i18n::tf!("{named}{described}\nThe low byte of the address.{mirror}") } else { crate::i18n::tf!("{named}{described}\nThe high byte of the address.{mirror}") };
            if let Some(r) = Self::stored(mnemonic)
                && let Some((value, from)) = self.held(line, r)
            {
                let expr = self.lines[from].operand.trim().trim_start_matches('#').to_string();
                out.push_str(&format!("\n${value:02X} ({expr}) written there."));
            }
            return Some(out);
        }
        let Some(r) = Self::stored(mnemonic) else {
            return Some(format!("{named}{}{mirror}", c64_hw::describe(register)));
        };
        let Some((value, from)) = self.held(line, r) else {
            return Some(crate::i18n::tf!("{named}{}{mirror}\n(The value written isn't known here.)", c64_hw::describe(register)));
        };
        let mut out = format!("{named}{}", c64_hw::describe_value(register, value));
        out.push_str(&format!("\n(from line {}){mirror}", from + 1));
        out.push_str(&self.more(line, register, value));
        Some(out)
    }

    /// What a value means with the rest of the source: a SID frequency or CIA timer from both its bytes, and
    /// where the VIC finds things with the bank chosen in $DD00.
    fn more(&self, line: usize, register: &Register, value: u8) -> String {
        let a = register.address;
        // The other byte of a pair, stored nearby.
        let pair = |other: u16| (line.saturating_sub(12)..(line + 12).min(self.lines.len())).filter(|n| *n != line).find_map(|n| {
            let l = &self.lines[n];
            let r = Self::stored(&l.mnemonic)?;
            (self.target(l).map(c64_hw::mirror) == Some(other)).then(|| self.held(n, r).map(|(v, _)| v)).flatten()
        });
        match a {
            0xD400..=0xD414 if (a - 0xD400) % 7 <= 1 => {
                let (low, high) = if (a - 0xD400).is_multiple_of(7) { (Some(value), pair(a + 1)) } else { (pair(a - 1), Some(value)) };
                match (low, high) {
                    (Some(low), Some(high)) => crate::i18n::tf!("\nWith the other byte: {}", c64_hw::explain_frequency(u16::from_le_bytes([low, high]))),
                    _ => String::new(),
                }
            }
            0xDC04..=0xDC07 | 0xDD04..=0xDD07 => {
                let (low, high) = if a & 1 == 0 { (Some(value), pair(a + 1)) } else { (pair(a - 1), Some(value)) };
                match (low, high) {
                    (Some(low), Some(high)) => crate::i18n::tf!("\nWith the other byte, timer {}: {}", if a & 2 == 0 { "A" } else { "B" }, c64_hw::explain_timer(u16::from_le_bytes([low, high]))),
                    _ => String::new(),
                }
            }
            0xD018 => {
                let dd00 = self.last_stored(0xDD00, self.lines.len());
                let bitmap = self.last_stored(0xD011, self.lines.len()).is_some_and(|v| v & 0x20 != 0);
                match dd00 {
                    Some(bank) => crate::i18n::tf!("\nWith $DD00 = ${bank:02X} (bank ${:04X}): {}", c64_hw::bank_of(bank), c64_hw::vic_memory(value, bank).words(bitmap)),
                    None => String::new(),
                }
            }
            0xD011 => {
                let d016 = self.last_stored(0xD016, self.lines.len()).unwrap_or(0xC8);
                format!("\nWith $D016 = ${d016:02X}: {}", c64_hw::screen_mode(value, d016))
            }
            _ => String::new(),
        }
    }

    fn describe_immediate(&self, line: usize, here: &Line) -> Option<String> {
        let register = match here.mnemonic.as_str() {
            "lda" => 'a',
            "ldx" => 'x',
            "ldy" => 'y',
            _ => return None,
        };
        let value = here.operand.trim().strip_prefix('#').and_then(|e| self.eval(e))? as u8;
        // The next store from that register, before it changes.
        for n in line + 1..(line + 16).min(self.lines.len()) {
            let l = &self.lines[n];
            if l.label {
                return None;
            }
            if Self::stored(&l.mnemonic) == Some(register)
                && let Some(address) = self.target(l)
                && let Some(found) = c64_hw::at(address)
                && found.access != Access::Call
                && (c64_hw::is_chip(address) || found.access == Access::Vector || found.chip == Chip::CpuPort && matches!(address_part(&l.operand), Some("$01" | "1")))
            {
                if found.access == Access::Vector {
                    return Some(crate::i18n::tf!("${value:02X}, stored on line {} to {} (${address:04X}), the {} byte of the {}", n + 1, found.name, if address == found.address { "low" } else { "high" }, found.title));
                }
                return Some(crate::i18n::tf!("Stored on line {} to:\n{}{}", n + 1, c64_hw::describe_value(found, value), self.more(n, found, value)));
            }
            if changes(&l.mnemonic, &l.operand).contains(&register) || matches!(l.mnemonic.as_str(), "rts" | "rti" | "jmp") {
                return None;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINES: &[&str] = &[
        "border = $d020",
        "        *= $0810",
        "start   sei",
        "        lda #$1b",
        "        sta $d011",
        "        lda #$35",
        "        sta $01",
        "        lda #$1f",
        "        sta $d418",
        "        lda #6",
        "        sta border",
        "        lda #<irq",
        "        sta $0314",
        "        lda #$45",
        "        sta $d400",
        "        lda #$1d",
        "        sta $d401",
        "        lda #$02",
        "        sta $dd00",
        "        lda #$18",
        "        sta $d018",
        "        jsr $ffd2",
        "        lda $d012",
        "irq     inc $d019",
        "        jmp $ea31",
    ];

    fn demo() -> String {
        LINES.join("\n")
    }

    fn source() -> Source {
        let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::M6502, std::path::Path::new("t.asm"), &demo()).assembly;
        Source::scan(&demo(), Some(&assembly))
    }

    fn hover(source: &Source, line: usize, find: &str) -> String {
        let text = LINES[line];
        let col = text.find(find).unwrap_or_else(|| panic!("{find} in {text}")) + 1;
        source.describe(line, text, col).unwrap_or_else(|| panic!("nothing over {find} in {text}"))
    }

    #[test]
    fn c64_source_is_told_from_other_6502_machines() {
        assert!(source().c64);
        assert!(!Source::scan("  lda #1\n  sta $d01a\n  sta wsync\n", None).c64, "the Atari's GTIA is at $D000 too");
        assert!(!Source::scan("  lda #1\n  sta $0400\n  rts\n", None).c64);
        assert!(Source::scan(":BasicUpstart2(start)\nstart: rts\n", None).c64);
    }

    #[test]
    fn registers_and_values_are_explained() {
        let s = source();
        let said = hover(&s, 4, "$d011");
        assert!(said.starts_with("SCROLY  $D011") && said.contains("$1B (%00011011) written: standard text, display on, 25 rows, scrolled down 3"), "{said}");
        assert!(said.contains("(from line 4)"), "{said}");
        let said = hover(&s, 3, "#$1b");
        assert!(said.starts_with("Stored on line 5 to:\nSCROLY"), "over the value, the register it goes to: {said}");
        let said = hover(&s, 6, "$01");
        assert!(said.contains("$A000-$BFFF RAM, $D000-$DFFF I/O, $E000-$FFFF RAM"), "{said}");
        let said = hover(&s, 8, "$d418");
        assert!(said.contains("volume 15, filter LP"), "{said}");
        let said = hover(&s, 10, "border");
        assert!(said.starts_with("border = $D020: EXTCOL") && said.contains("6 blue"), "{said}");
        assert!(hover(&s, 0, "border").starts_with("border = $D020\nEXTCOL"), "where it's defined");
        let said = hover(&s, 12, "$0314");
        assert!(said.contains("CINV") && said.contains("low byte") && said.contains("(<irq) written there"), "{said}");
        let said = hover(&s, 16, "$d401");
        assert!(said.contains("With the other byte: frequency $1D45: PAL 440.0 Hz, A4"), "{said}");
        let said = hover(&s, 20, "$d018");
        assert!(said.contains("With $DD00 = $02 (bank $4000): screen $4400-$47E7, characters $6000-$67FF"), "{said}");
        let said = hover(&s, 21, "$ffd2");
        assert!(said.starts_with("CHROUT  $FFD2  Write a character"), "{said}");
        assert!(hover(&s, 21, "jsr").starts_with("CHROUT"));
        assert!(hover(&s, 22, "$d012").starts_with("RASTER  $D012"));
        assert!(hover(&s, 24, "$ea31").contains("IRQ handler"));
        assert!(s.describe(9, LINES[9], 13).unwrap().contains("Stored on line 11 to:\nEXTCOL"));
    }

    #[test]
    fn kickassembler_and_64tass_source_gets_hover_too() {
        let text = ".const BORDER = $d020\n* = $c000 \"Code\"\nstart: lda #2\n  sta BORDER\n!loop: dex\n  bne !loop-\nclear: {\nloop: rts\n}\n";
        let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::M6502, std::path::Path::new("k.asm"), text).assembly;
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let intel = crate::code_intel::CodeIntel { c64: Some(std::sync::Arc::new(Source::scan(text, Some(&assembly)))), assembly: Some(std::sync::Arc::new(assembly)), ..crate::code_intel::CodeIntel::for_cpu(crate::assemblers::Cpu::M6502) };
        let said = intel.describe(3, "  sta BORDER", 8).unwrap();
        assert!(said.starts_with("BORDER = $D020: EXTCOL") && said.contains("2 red"), "{said}");
        assert_eq!(intel.describe(5, "  bne !loop-", 8).as_deref(), Some("loop = 49157 ($C005)\nDefined on line 5"));
        assert_eq!(intel.describe(7, "loop: rts", 2).as_deref(), Some("clear.loop = 49160 ($C008)\nDefined on line 8"));
        let text = "        *= $c000\nborder  = $d020\nstart   lda #2\n        sta border\nshow    .proc\nloop    rts\n        .pend\n        jsr show\n";
        let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::M6502, std::path::Path::new("t.asm"), text).assembly;
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let intel = crate::code_intel::CodeIntel { c64: Some(std::sync::Arc::new(Source::scan(text, Some(&assembly)))), assembly: Some(std::sync::Arc::new(assembly)), ..crate::code_intel::CodeIntel::for_cpu(crate::assemblers::Cpu::M6502) };
        assert!(intel.describe(3, "        sta border", 14).unwrap().contains("2 red"));
        assert_eq!(intel.describe(5, "loop    rts", 1).as_deref(), Some("show.loop = 49157 ($C005)\nDefined on line 6"));
    }

    #[test]
    fn hover_comes_through_code_intel_and_stays_off_68000_code() {
        let text = demo();
        let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::M6502, std::path::Path::new("t.asm"), &text).assembly;
        let intel = crate::code_intel::CodeIntel { c64: Some(std::sync::Arc::new(Source::scan(&text, Some(&assembly)))), assembly: Some(std::sync::Arc::new(assembly)), ..crate::code_intel::CodeIntel::for_cpu(crate::assemblers::Cpu::M6502) };
        let line = LINES[4];
        assert!(intel.describe(4, line, line.find("$d011").unwrap() + 1).unwrap().starts_with("SCROLY"));
        assert!(intel.describe(4, "        sta $d011 ; $d020", 22).is_none(), "nothing in a comment");
        // 68000 code for the Atari ST or the Amiga gets nothing of the C64's, and 6502 code nothing of theirs.
        let st = "\tmove.w\t#$777,$ff8240\n\tmove.b\t#1,$d020\n";
        assert!(!Source::scan(st, None).c64);
        let amiga = crate::amiga_source::Source::scan(&text, None);
        let atari = crate::st_source::Source::scan(&text, None);
        let six = crate::code_intel::CodeIntel { hardware: Some(std::sync::Arc::new(amiga)), atari: Some(std::sync::Arc::new(atari)), ..crate::code_intel::CodeIntel::for_cpu(crate::assemblers::Cpu::M6502) };
        assert!(six.describe(4, line, line.find("$d011").unwrap() + 1).is_none_or(|said| !said.contains("COLOR") && !said.contains("custom")));
    }
}
