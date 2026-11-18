//! The ARM of the Acorn Archimedes: the ARM2 (ARMv2), and the ARM3's SWP (ARMv2a), in Acorn's syntax as its
//! ObjAsm and BBC BASIC's assembler write it. Every instruction has its condition (ADDEQ, LDMNEFD), S to set the
//! flags (ADDEQS, or ADDSEQ as later assemblers write it), and P where the 26-bit ARM's TEQP and its kin set the
//! PSR in R15. A number an instruction can't hold is tried the other way round, as Acorn's assemblers do: MOV R0,#-1
//! is MVN R0,#0, and ADD R0,R0,#-4 is SUB R0,R0,#4. SWI takes a number or a RISC OS SWI's name.

use super::{Isa, Line};

pub struct Arm;

pub const CONDITIONS: [&str; 16] = ["eq", "ne", "cs", "cc", "mi", "pl", "vs", "vc", "hi", "ls", "ge", "lt", "gt", "le", "al", "nv"];

/// Data processing, by their opcodes.
pub const DATA: [&str; 16] = ["and", "eor", "sub", "rsb", "add", "adc", "sbc", "rsc", "tst", "teq", "cmp", "cmn", "orr", "mov", "bic", "mvn"];

pub const SHIFTS: [&str; 4] = ["lsl", "lsr", "asr", "ror"];

/// RISC OS's own SWIs, by number: the operating system's, as the Programmer's Reference Manual lists them.
pub const SWIS: &[(&str, u32)] = &[
    ("OS_WriteC", 0x00),
    ("OS_WriteS", 0x01),
    ("OS_Write0", 0x02),
    ("OS_NewLine", 0x03),
    ("OS_ReadC", 0x04),
    ("OS_CLI", 0x05),
    ("OS_Byte", 0x06),
    ("OS_Word", 0x07),
    ("OS_File", 0x08),
    ("OS_Args", 0x09),
    ("OS_BGet", 0x0A),
    ("OS_BPut", 0x0B),
    ("OS_GBPB", 0x0C),
    ("OS_Find", 0x0D),
    ("OS_ReadLine", 0x0E),
    ("OS_Control", 0x0F),
    ("OS_GetEnv", 0x10),
    ("OS_Exit", 0x11),
    ("OS_SetEnv", 0x12),
    ("OS_IntOn", 0x13),
    ("OS_IntOff", 0x14),
    ("OS_CallBack", 0x15),
    ("OS_EnterOS", 0x16),
    ("OS_BreakPt", 0x17),
    ("OS_BreakCtrl", 0x18),
    ("OS_UnusedSWI", 0x19),
    ("OS_UpdateMEMC", 0x1A),
    ("OS_SetCallBack", 0x1B),
    ("OS_Mouse", 0x1C),
    ("OS_Heap", 0x1D),
    ("OS_Module", 0x1E),
    ("OS_Claim", 0x1F),
    ("OS_Release", 0x20),
    ("OS_ReadUnsigned", 0x21),
    ("OS_GenerateEvent", 0x22),
    ("OS_ReadVarVal", 0x23),
    ("OS_SetVarVal", 0x24),
    ("OS_GSInit", 0x25),
    ("OS_GSRead", 0x26),
    ("OS_GSTrans", 0x27),
    ("OS_BinaryToDecimal", 0x28),
    ("OS_FSControl", 0x29),
    ("OS_ChangeDynamicArea", 0x2A),
    ("OS_GenerateError", 0x2B),
    ("OS_ReadEscapeState", 0x2C),
    ("OS_EvaluateExpression", 0x2D),
    ("OS_SpriteOp", 0x2E),
    ("OS_ReadPalette", 0x2F),
    ("OS_ServiceCall", 0x30),
    ("OS_ReadVduVariables", 0x31),
    ("OS_ReadPoint", 0x32),
    ("OS_UpCall", 0x33),
    ("OS_CallAVector", 0x34),
    ("OS_ReadModeVariable", 0x35),
    ("OS_RemoveCursors", 0x36),
    ("OS_RestoreCursors", 0x37),
    ("OS_SWINumberToString", 0x38),
    ("OS_SWINumberFromString", 0x39),
    ("OS_ValidateAddress", 0x3A),
    ("OS_CallAfter", 0x3B),
    ("OS_CallEvery", 0x3C),
    ("OS_RemoveTickerEvent", 0x3D),
    ("OS_InstallKeyHandler", 0x3E),
    ("OS_CheckModeValid", 0x3F),
    ("OS_ChangeEnvironment", 0x40),
    ("OS_ClaimScreenMemory", 0x41),
    ("OS_ReadMonotonicTime", 0x42),
    ("OS_SubstituteArgs", 0x43),
    ("OS_PrettyPrint", 0x44),
    ("OS_Plot", 0x45),
    ("OS_WriteN", 0x46),
];

/// A SWI's number by its name: X before it (XOS_WriteC) is the form that returns errors rather than raising them;
/// OS_WriteI+n writes the character n.
pub fn swi_number(name: &str) -> Option<u32> {
    let (x, name) = match name.strip_prefix('X') {
        Some(rest) if rest.contains('_') => (0x20000, rest),
        _ => (0, name),
    };
    if let Some(rest) = name.strip_prefix("OS_WriteI") {
        let n = rest.trim_start_matches('+').trim();
        let c = if n.is_empty() { 0 } else if let Some(ch) = n.strip_prefix('"').and_then(|s| s.strip_suffix('"')).filter(|s| s.chars().count() == 1) { ch.chars().next()? as u32 } else { parse_number(n)? };
        return (c < 256).then_some(0x100 + c + x);
    }
    SWIS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v + x)
}

/// A SWI's name by its number, for the disassembler.
pub fn swi_name(number: u32) -> Option<String> {
    let x = if number & 0x20000 != 0 { "X" } else { "" };
    let base = number & !0x20000;
    if (0x100..0x200).contains(&base) {
        let c = base - 0x100;
        return Some(format!("{x}OS_WriteI+{c}"));
    }
    SWIS.iter().find(|(_, v)| *v == base).map(|(n, _)| format!("{x}{n}"))
}

fn parse_number(text: &str) -> Option<u32> {
    let t = text.trim();
    if let Some(h) = t.strip_prefix('&').or_else(|| t.strip_prefix("0x")).or_else(|| t.strip_prefix('$')) {
        u32::from_str_radix(h, 16).ok()
    } else {
        t.parse().ok()
    }
}

/// A register's number: R0 to R15, and the names APCS gives them.
pub fn register(text: &str) -> Option<u32> {
    let t = text.trim().to_ascii_lowercase();
    if let Some(n) = t.strip_prefix('r').and_then(|n| n.parse::<u32>().ok()).filter(|n| *n < 16) {
        return Some(n);
    }
    Some(match t.as_str() {
        "a1" => 0,
        "a2" => 1,
        "a3" => 2,
        "a4" => 3,
        "v1" => 4,
        "v2" => 5,
        "v3" => 6,
        "v4" => 7,
        "v5" => 8,
        "v6" => 9,
        "sl" => 10,
        "fp" => 11,
        "ip" => 12,
        "sp" => 13,
        "lr" => 14,
        "pc" => 15,
        _ => return None,
    })
}

/// A mnemonic taken apart: what it is, its condition, and the letters after it (S, P, B, T, the LDM/STM modes).
#[derive(Debug, Clone, PartialEq)]
struct Parts {
    base: &'static str,
    cond: u32,
    flags: String,
}

/// The mnemonics' bases, longest first, so BL is tried before B.
const BASES: &[&str] = &[
    "and", "eor", "sub", "rsb", "add", "adc", "sbc", "rsc", "tst", "teq", "cmp", "cmn", "orr", "mov", "bic", "mvn", "mul", "mla", "ldr", "str", "ldm", "stm", "swp", "swi", "svc", "bl", "b", "adr", "nop", "cdp", "ldc", "stc", "mcr", "mrc",
];

/// The letters each base can have after it.
fn allowed(base: &str, flags: &str) -> bool {
    let f = flags;
    match base {
        "and" | "eor" | "sub" | "rsb" | "add" | "adc" | "sbc" | "rsc" | "orr" | "mov" | "bic" | "mvn" | "mul" | "mla" => f.is_empty() || f == "s",
        "tst" | "teq" | "cmp" | "cmn" => f.is_empty() || f == "s" || f == "p",
        "ldr" | "str" => matches!(f, "" | "b" | "t" | "bt"),
        "ldm" | "stm" => matches!(f, "ia" | "ib" | "da" | "db" | "fd" | "ed" | "fa" | "ea"),
        "swp" => f.is_empty() || f == "b",
        "ldc" | "stc" => f.is_empty() || f == "l",
        _ => f.is_empty(),
    }
}

fn split_mnemonic(word: &str) -> Option<Parts> {
    let word = word.to_ascii_lowercase();
    for base in BASES {
        let Some(rest) = word.strip_prefix(base) else { continue };
        let cond_at = |s: &str| CONDITIONS.iter().position(|c| *c == s).map(|n| n as u32);
        // Acorn's order: the condition, then the letters (LDMEQFD, ADDEQS); and the later one: letters, then
        // the condition (ADDSEQ, LDMFDEQ).
        let mut tries: Vec<(u32, String)> = vec![(14, rest.to_string())];
        if rest.len() >= 2 {
            if let Some(c) = cond_at(&rest[..2]) {
                tries.push((c, rest[2..].to_string()));
            }
            if let Some(c) = cond_at(&rest[rest.len() - 2..]) {
                tries.push((c, rest[..rest.len() - 2].to_string()));
            }
        }
        for (cond, flags) in tries {
            if allowed(base, &flags) {
                return Some(Parts { base, cond, flags });
            }
        }
    }
    None
}

impl Isa for Arm {
    fn name(&self) -> &'static str {
        "ARM"
    }

    fn is_instruction(&self, word: &str) -> bool {
        split_mnemonic(word).is_some()
    }

    fn is_directive(&self, word: &str) -> bool {
        matches!(word.to_ascii_lowercase().as_str(), "dcb" | "dcw" | "dcd" | "equb" | "equw" | "equd" | "equs" | "align" | "area" | "entry" | "cpu" | "%")
    }

    fn directive(&self, word: &str, operands: &str, line: &mut Line) -> Option<Result<Vec<u8>, String>> {
        let unit = match word {
            "dcb" | "equb" | "equs" | "=" => 1,
            "dcw" | "equw" => 2,
            "dcd" | "equd" | "&" => 4,
            "align" => {
                let to = if operands.trim().is_empty() { 4 } else { line.eval(operands).ok()?.max(1) };
                let here = line.here();
                return Some(Ok(vec![0; ((to - here.rem_euclid(to)) % to) as usize]));
            }
            // ObjAsm's AREA and ENTRY: one area, which starts where the code does.
            "area" | "entry" => return Some(Ok(Vec::new())),
            "cpu" => {
                let level = match operands.trim().to_ascii_lowercase().as_str() {
                    "arm2" | "armv2" => 2,
                    "arm3" | "armv2a" | "arm250" => 3,
                    other => return Some(Err(crate::i18n::tf!("CPU {other} isn't one Limoniter's ARM assembler knows: ARM2 or ARM3"))),
                };
                line.set_setting("arm", level);
                return Some(Ok(Vec::new()));
            }
            // BBC BASIC's % reserves bytes.
            "%" => return Some(line.eval(operands).map(|n| vec![0; n.clamp(0, 0x100_0000) as usize])),
            _ => return None,
        };
        let mut out = Vec::new();
        for item in split(operands) {
            // A string, as DCB "text" or EQUS "text" has it.
            if let Some(text) = item.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
                out.extend(text.replace("\"\"", "\"").bytes());
                continue;
            }
            match line.eval(&item) {
                Ok(v) => out.extend(&v.to_le_bytes()[..unit]),
                Err(problem) => return Some(Err(problem)),
            }
        }
        Some(Ok(out))
    }

    fn encode(&self, mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u8>, String> {
        let parts = split_mnemonic(mnemonic).ok_or_else(|| crate::i18n::tf!("{} isn't an ARM instruction", mnemonic.to_ascii_uppercase()))?;
        let words = encode(&parts, operands, line)?;
        let level = line.setting("arm").unwrap_or(3);
        if parts.base == "swp" && level < 3 {
            return Err(crate::i18n::tr("SWP is the ARM3's: the ARM2 doesn't have it (CPU ARM3)").into());
        }
        Ok(words.iter().flat_map(|w| w.to_le_bytes()).collect())
    }
}

/// Splits operands at commas that aren't inside brackets, braces or quotes.
fn split(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, false, 0);
    for (i, c) in text.char_indices() {
        match c {
            '"' => quote = !quote,
            '[' | '{' | '(' if !quote => depth += 1,
            ']' | '}' | ')' if !quote => depth -= 1,
            ',' if !quote && depth == 0 => {
                out.push(text[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() || !out.is_empty() {
        out.push(last.to_string());
    }
    out
}

fn reg(text: &str) -> Result<u32, String> {
    register(text).ok_or_else(|| crate::i18n::tf!("{} isn't a register (R0 to R15)", text.trim()))
}

/// An 8-bit value rotated right by an even amount: the immediate a data processing instruction holds.
pub fn encode_immediate(value: u32) -> Option<u32> {
    for rotate in 0..16u32 {
        let unrotated = value.rotate_left(rotate * 2);
        if unrotated <= 0xFF {
            return Some((rotate << 8) | unrotated);
        }
    }
    None
}

/// An immediate's value: #5, #&FF, or ObjAsm's #imm8,rotation (as the disassembler writes a value held unusually).
fn immediate(text: &str, line: &mut Line) -> Result<i64, String> {
    line.eval(text.trim().trim_start_matches('#'))
}

/// A shift on a register: LSL #n, LSR Rs, RRX and the rest, as bits 4 to 11 of the instruction hold it.
fn shift(text: &str, line: &mut Line) -> Result<u32, String> {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    if lower == "rrx" {
        return Ok(3 << 5);
    }
    let (name, amount) = lower.split_at(3.min(lower.len()));
    let kind = match name {
        "lsl" | "asl" => 0,
        "lsr" => 1,
        "asr" => 2,
        "ror" => 3,
        _ => return Err(crate::i18n::tf!("{t} isn't a shift: LSL, LSR, ASR, ROR or RRX")),
    };
    let amount = amount.trim();
    if let Some(r) = register(amount) {
        return Ok((r << 8) | (kind << 5) | (1 << 4));
    }
    let n = immediate(amount, line)?;
    // LSR #32 and ASR #32 are written as 0; LSL #0 is no shift; ROR #0 would be RRX.
    let n = match (kind, n) {
        (1 | 2, 32) => 0,
        (_, n) if (0..32).contains(&n) => n as u32,
        _ => return Err(crate::i18n::tf!("{t}: a shift is by 0 to 31 (or 32 for LSR and ASR)")),
    };
    if kind == 3 && n == 0 {
        return Err(crate::i18n::tr("ROR #0 isn't a shift: RRX is the one-bit rotate through carry").into());
    }
    Ok((n << 7) | (kind << 5))
}

/// Operand 2: an immediate (with the I bit), or a register shifted perhaps.
fn operand2(items: &[String], line: &mut Line) -> Result<(u32, Option<i64>), String> {
    let first = items.first().ok_or_else(|| crate::i18n::tr("an operand is missing"))?;
    if first.trim_start().starts_with('#') {
        // #imm8,rotation: ObjAsm's way of saying exactly how a value is held.
        if let Some(rotation) = items.get(1) {
            let value = immediate(first, line)?;
            let rotation = immediate(rotation, line)?;
            if !(0..256).contains(&value) || !(0..32).contains(&rotation) || rotation % 2 != 0 {
                return Err(crate::i18n::tr("#value,rotation is an 8-bit value and an even rotation up to 30").into());
            }
            return Ok(((1 << 25) | (((rotation / 2) as u32) << 8) | value as u32, None));
        }
        let value = immediate(first, line)?;
        return Ok((1 << 25, Some(value)));
    }
    let rm = reg(first)?;
    let shifted = match items.get(1) {
        Some(s) => shift(s, line)?,
        None => 0,
    };
    Ok((shifted | rm, None))
}

fn encode(parts: &Parts, operands: &str, line: &mut Line) -> Result<Vec<u32>, String> {
    let name = format!("{}{}", parts.base, parts.flags).to_ascii_uppercase();
    let items = split(operands);
    let cond = parts.cond << 28;
    let s_bit = if parts.flags == "s" || parts.flags == "p" { 1 << 20 } else { 0 };
    match parts.base {
        "nop" => Ok(vec![cond | 0x01A0_0000]),
        base if DATA.contains(&base) => {
            let mut opcode = DATA.iter().position(|d| *d == base).expect("found") as u32;
            let compare = (8..12).contains(&opcode);
            let moves = opcode == 13 || opcode == 15;
            let (rd, rn, rest) = if compare {
                // TEQP and its kin write the PSR: Rd is R15.
                (if parts.flags == "p" { 15 } else { 0 }, reg(items.first().ok_or_else(|| crate::i18n::tf!("{name} needs a register"))?)?, &items[1.min(items.len())..])
            } else if moves {
                (reg(items.first().ok_or_else(|| crate::i18n::tf!("{name} needs a register"))?)?, 0, &items[1.min(items.len())..])
            } else {
                if items.len() < 3 {
                    return Err(crate::i18n::tf!("{name} needs a register, a register and an operand"));
                }
                (reg(&items[0])?, reg(&items[1])?, &items[2..])
            };
            if rest.is_empty() {
                return Err(crate::i18n::tf!("{name} needs an operand after its registers"));
            }
            let (mut op2, value) = operand2(rest, line)?;
            if let Some(v) = value {
                let v32 = v as u32;
                // Held as it is, or the other way round with the opposite instruction: MOV and MVN, ADD and SUB,
                // CMP and CMN, AND and BIC, ADC and SBC.
                let other = |op: u32| -> Option<(u32, u32)> {
                    match op {
                        13 => Some((15, !v32)),
                        15 => Some((13, !v32)),
                        4 => Some((2, v32.wrapping_neg())),
                        2 => Some((4, v32.wrapping_neg())),
                        10 => Some((11, v32.wrapping_neg())),
                        11 => Some((10, v32.wrapping_neg())),
                        0 => Some((14, !v32)),
                        14 => Some((0, !v32)),
                        5 => Some((6, !v32)),
                        6 => Some((5, !v32)),
                        _ => None,
                    }
                };
                match encode_immediate(v32) {
                    Some(imm) => op2 |= imm,
                    None => match other(opcode).and_then(|(op, value)| encode_immediate(value).map(|imm| (op, imm))) {
                        Some((op, imm)) => {
                            opcode = op;
                            op2 |= imm;
                        }
                        None if line.checking() => return Err(crate::i18n::tf!("{name} can't hold #{v} (&{v32:X}): an instruction holds 8 bits rotated by an even amount")),
                        None => {}
                    },
                }
            }
            let s = if compare { 1 << 20 } else { s_bit };
            Ok(vec![cond | (opcode << 21) | s | (rn << 16) | (rd << 12) | op2])
        }
        "mul" | "mla" => {
            let n = if parts.base == "mla" { 4 } else { 3 };
            if items.len() != n {
                return Err(crate::i18n::tf!("{name} takes {n} registers"));
            }
            let rd = reg(&items[0])?;
            let rm = reg(&items[1])?;
            let rs = reg(&items[2])?;
            let rn = if n == 4 { reg(&items[3])? } else { 0 };
            if rd == rm {
                line.warn(crate::i18n::tf!("{name}'s destination and first register are the same, which the ARM2 and ARM3 get wrong: swap the two registers it multiplies"));
            }
            let a = if n == 4 { 1 << 21 } else { 0 };
            Ok(vec![cond | a | s_bit | (rd << 16) | (rn << 12) | (rs << 8) | 0x90 | rm])
        }
        "swp" => {
            if items.len() != 3 {
                return Err(crate::i18n::tr("SWP takes a register, a register and [a register]").into());
            }
            let rd = reg(&items[0])?;
            let rm = reg(&items[1])?;
            let rn = reg(items[2].trim().trim_start_matches('[').trim_end_matches(']'))?;
            let b = if parts.flags == "b" { 1 << 22 } else { 0 };
            Ok(vec![cond | 0x0100_0090 | b | (rn << 16) | (rd << 12) | rm])
        }
        "ldr" | "str" => transfer(parts, &items, cond, line),
        "ldm" | "stm" => multiple(parts, &items, cond, line),
        "b" | "bl" => {
            let target = line.eval(items.first().ok_or_else(|| crate::i18n::tf!("{name} needs where to go"))?)?;
            // The ARM2's and ARM3's addresses are 26 bits, and a branch wraps round them.
            let offset = (target - (line.here() + 8)).rem_euclid(1 << 26);
            let offset = if offset >= 1 << 25 { offset - (1 << 26) } else { offset };
            if line.checking() && offset % 4 != 0 {
                return Err(crate::i18n::tf!("{name} can't go to {}: a branch goes to a whole word", items[0]));
            }
            let l = if parts.base == "bl" { 1 << 24 } else { 0 };
            Ok(vec![cond | 0x0A00_0000 | l | (((offset >> 2) as u32) & 0x00FF_FFFF)])
        }
        "swi" | "svc" => {
            let text = items.first().ok_or_else(|| crate::i18n::tr("SWI needs a number or a SWI's name"))?.trim();
            let named = text.trim_matches('"');
            let number = match swi_number(named) {
                Some(n) => i64::from(n),
                None if text.starts_with('"') => return Err(crate::i18n::tf!("{named} isn't a SWI Limoniter knows: give its number")),
                None => line.eval(text)?,
            };
            if line.checking() && !(0..1 << 24).contains(&number) {
                return Err(crate::i18n::tf!("{number} is too big for a SWI's number (24 bits)"));
            }
            Ok(vec![cond | 0x0F00_0000 | (number as u32 & 0x00FF_FFFF)])
        }
        "adr" => {
            if items.len() != 2 {
                return Err(crate::i18n::tr("ADR takes a register and a label").into());
            }
            let rd = reg(&items[0])?;
            let target = line.eval(&items[1])?;
            let offset = target - (line.here() + 8);
            let (opcode, magnitude) = if offset >= 0 { (4, offset as u32) } else { (2, offset.unsigned_abs() as u32) };
            let imm = match encode_immediate(magnitude) {
                Some(imm) => imm,
                None if line.checking() => return Err(crate::i18n::tf!("ADR can't reach {}: it's {offset} bytes away, which an ADD or SUB from PC can't hold", items[1])),
                None => 0,
            };
            Ok(vec![cond | (1 << 25) | (opcode << 21) | (15 << 16) | (rd << 12) | imm])
        }
        "mcr" | "mrc" | "cdp" => coprocessor(parts, &items, cond, line),
        "ldc" | "stc" => coprocessor_transfer(parts, &items, cond, line),
        _ => Err(crate::i18n::tf!("{name} isn't an ARM instruction Limoniter knows")),
    }
}

/// LDR and STR, with B for a byte and T to use the user's address space.
fn transfer(parts: &Parts, items: &[String], cond: u32, line: &mut Line) -> Result<Vec<u32>, String> {
    let name = format!("{}{}", parts.base, parts.flags).to_ascii_uppercase();
    if items.len() < 2 {
        return Err(crate::i18n::tf!("{name} takes a register and an address"));
    }
    let rd = reg(&items[0])?;
    let l = if parts.base == "ldr" { 1 << 20 } else { 0 };
    let b = if parts.flags.contains('b') { 1 << 22 } else { 0 };
    let t = parts.flags.contains('t');
    let base = cond | 0x0400_0000 | l | b | (rd << 12);
    let address = items[1].trim();
    // A label: from PC, as LDR R0,value.
    if !address.starts_with('[') {
        let target = line.eval(address)?;
        let offset = target - (line.here() + 8);
        if line.checking() && offset.abs() > 4095 {
            return Err(crate::i18n::tf!("{name} can't reach {address}: it's {offset} bytes away, more than 4095"));
        }
        let u = if offset >= 0 { 1 << 23 } else { 0 };
        return Ok(vec![base | (1 << 24) | u | (15 << 16) | (offset.unsigned_abs() as u32 & 0xFFF)]);
    }
    let close = address.find(']').ok_or_else(|| crate::i18n::tf!("{address} isn't closed with ]"))?;
    let inside = split(&address[1..close]);
    let rn = reg(inside.first().ok_or_else(|| crate::i18n::tr("an address needs a base register"))?)?;
    let after_bracket = address[close + 1..].trim();
    let write_back = after_bracket == "!";
    // [Rn,offset]{!}, or [Rn],offset after it.
    let (pre, offset_items): (bool, Vec<String>) = if inside.len() > 1 { (true, inside[1..].to_vec()) } else { (items.len() == 2, items[2..].to_vec()) };
    if t && pre {
        return Err(crate::i18n::tf!("{name} only goes with a post-indexed address: [Rn],offset"));
    }
    let p = if pre { 1 << 24 } else { 0 };
    // Post-indexing always writes back; T is its W bit.
    let w = if (pre && write_back) || (!pre && t) { 1 << 21 } else { 0 };
    let mut word = base | p | w | (rn << 16);
    match offset_items.first() {
        None => word |= 1 << 23,
        Some(first) if first.trim_start().starts_with('#') => {
            let v = immediate(first, line)?;
            if line.checking() && v.abs() > 4095 {
                return Err(crate::i18n::tf!("{name}'s offset is {v}: it holds up to 4095 either way"));
            }
            word |= if v >= 0 { 1 << 23 } else { 0 } | (v.unsigned_abs() as u32 & 0xFFF);
        }
        Some(first) => {
            let (negative, register_text) = match first.trim().strip_prefix('-') {
                Some(r) => (true, r),
                None => (false, first.trim().trim_start_matches('+')),
            };
            let rm = reg(register_text)?;
            let shifted = match offset_items.get(1) {
                Some(s) => shift(s, line)?,
                None => 0,
            };
            if shifted & (1 << 4) != 0 {
                return Err(crate::i18n::tf!("{name}'s offset can be shifted by a number, not by a register"));
            }
            word |= (1 << 25) | if negative { 0 } else { 1 << 23 } | shifted | rm;
        }
    }
    Ok(vec![word])
}

/// LDM and STM: a base register, perhaps written back, and a list of registers, perhaps with ^.
fn multiple(parts: &Parts, items: &[String], cond: u32, _line: &mut Line) -> Result<Vec<u32>, String> {
    let name = format!("{}{}", parts.base, parts.flags).to_ascii_uppercase();
    if items.len() != 2 {
        return Err(crate::i18n::tf!("{name} takes a base register and a list of registers in {{ }}"));
    }
    let load = parts.base == "ldm";
    let (p, u) = match (parts.flags.as_str(), load) {
        ("ia", _) | ("fd", true) | ("ea", false) => (0, 1),
        ("ib", _) | ("ed", true) | ("fa", false) => (1, 1),
        ("da", _) | ("fa", true) | ("ed", false) => (0, 0),
        _ => (1, 0),
    };
    let base_text = items[0].trim();
    let (base_text, w) = match base_text.strip_suffix('!') {
        Some(b) => (b, 1),
        None => (base_text, 0),
    };
    let rn = reg(base_text)?;
    let list_text = items[1].trim();
    let (list_text, s) = match list_text.strip_suffix('^') {
        Some(l) => (l.trim(), 1),
        None => (list_text, 0),
    };
    let inner = list_text.strip_prefix('{').and_then(|l| l.strip_suffix('}')).ok_or_else(|| crate::i18n::tf!("{name}'s registers go in {{ }}"))?;
    let mut list = 0u32;
    for piece in inner.split(',') {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        match piece.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (reg(a)?, reg(b)?);
                if a > b {
                    return Err(crate::i18n::tf!("{piece} runs backwards"));
                }
                for r in a..=b {
                    list |= 1 << r;
                }
            }
            None => list |= 1 << reg(piece)?,
        }
    }
    if list == 0 {
        return Err(crate::i18n::tf!("{name} needs at least one register"));
    }
    Ok(vec![cond | 0x0800_0000 | (p << 24) | (u << 23) | (s << 22) | (w << 21) | (if load { 1 << 20 } else { 0 }) | (rn << 16) | list])
}

fn coprocessor_number(text: &str) -> Result<u32, String> {
    let t = text.trim().to_ascii_lowercase();
    t.strip_prefix('p').and_then(|n| n.parse::<u32>().ok()).filter(|n| *n < 16).ok_or_else(|| crate::i18n::tf!("{text} isn't a coprocessor (P0 to P15)"))
}

fn coprocessor_register(text: &str) -> Result<u32, String> {
    let t = text.trim().to_ascii_lowercase();
    t.strip_prefix('c').and_then(|n| n.parse::<u32>().ok()).filter(|n| *n < 16).ok_or_else(|| crate::i18n::tf!("{text} isn't a coprocessor's register (C0 to C15)"))
}

/// MCR, MRC and CDP: the coprocessor, its opcode, and registers.
fn coprocessor(parts: &Parts, items: &[String], cond: u32, line: &mut Line) -> Result<Vec<u32>, String> {
    let name = parts.base.to_ascii_uppercase();
    if !(5..=6).contains(&items.len()) {
        return Err(crate::i18n::tf!("{name} takes a coprocessor, an opcode, three registers and perhaps another opcode"));
    }
    let cp = coprocessor_number(&items[0])?;
    let op1 = line.eval(&items[1])? as u32;
    let op2 = match items.get(5) {
        Some(o) => line.eval(o)? as u32,
        None => 0,
    };
    let crn = coprocessor_register(&items[3])?;
    let crm = coprocessor_register(&items[4])?;
    Ok(vec![match parts.base {
        "cdp" => {
            let crd = coprocessor_register(&items[2])?;
            cond | 0x0E00_0000 | ((op1 & 15) << 20) | (crn << 16) | (crd << 12) | (cp << 8) | ((op2 & 7) << 5) | crm
        }
        base => {
            let rd = reg(&items[2])?;
            let l = if base == "mrc" { 1 << 20 } else { 0 };
            cond | 0x0E00_0010 | ((op1 & 7) << 21) | l | (crn << 16) | (rd << 12) | (cp << 8) | ((op2 & 7) << 5) | crm
        }
    }])
}

/// LDC and STC: a coprocessor's register to or from memory.
fn coprocessor_transfer(parts: &Parts, items: &[String], cond: u32, line: &mut Line) -> Result<Vec<u32>, String> {
    let name = parts.base.to_ascii_uppercase();
    if items.len() < 3 {
        return Err(crate::i18n::tf!("{name} takes a coprocessor, its register and an address"));
    }
    let cp = coprocessor_number(&items[0])?;
    let crd = coprocessor_register(&items[1])?;
    let address = items[2].trim();
    let close = address.find(']').ok_or_else(|| crate::i18n::tf!("{address} isn't closed with ]"))?;
    let inside = split(&address[1..close]);
    let rn = reg(inside.first().ok_or_else(|| crate::i18n::tr("an address needs a base register"))?)?;
    let write_back = address[close + 1..].trim() == "!";
    let (pre, offset) = match (inside.get(1), items.get(3)) {
        (Some(o), _) => (true, Some(o.clone())),
        (None, Some(o)) => (false, Some(o.clone())),
        (None, None) => (true, None),
    };
    let v = match offset {
        Some(o) => immediate(&o, line)?,
        None => 0,
    };
    if line.checking() && (v % 4 != 0 || v.abs() > 1020) {
        return Err(crate::i18n::tf!("{name}'s offset is a multiple of 4 up to 1020 either way"));
    }
    let l = if parts.base == "ldc" { 1 << 20 } else { 0 };
    let n = if parts.flags == "l" { 1 << 22 } else { 0 };
    let p = if pre { 1 << 24 } else { 0 };
    let w = if write_back || !pre { 1 << 21 } else { 0 };
    let u = if v >= 0 { 1 << 23 } else { 0 };
    Ok(vec![cond | 0x0C00_0000 | p | u | n | w | l | (rn << 16) | (crd << 12) | (cp << 8) | ((v.unsigned_abs() as u32 / 4) & 0xFF)])
}

#[cfg(test)]
mod tests;
