//! The 6502 (and the 6510 in the Commodore 64), with the undocumented instructions C64 programs use: LAX, SAX,
//! DCP, ISC, SLO, RLA, SRE, RRA, ANC, ALR, ARR and SBX. Zero page is used wherever an address is known to fit.

use super::{Isa, Line};

pub struct M6502;

/// How an instruction finds its operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Implied,
    Accumulator,
    Immediate,
    ZeroPage,
    ZeroPageX,
    ZeroPageY,
    Absolute,
    AbsoluteX,
    AbsoluteY,
    Indirect,
    IndirectX,
    IndirectY,
    Relative,
}

use Mode::*;

/// Each instruction's opcodes by mode.
const OPCODES: &[(&str, &[(Mode, u8)])] = &[
    ("adc", &[(Immediate, 0x69), (ZeroPage, 0x65), (ZeroPageX, 0x75), (Absolute, 0x6D), (AbsoluteX, 0x7D), (AbsoluteY, 0x79), (IndirectX, 0x61), (IndirectY, 0x71)]),
    ("and", &[(Immediate, 0x29), (ZeroPage, 0x25), (ZeroPageX, 0x35), (Absolute, 0x2D), (AbsoluteX, 0x3D), (AbsoluteY, 0x39), (IndirectX, 0x21), (IndirectY, 0x31)]),
    ("asl", &[(Accumulator, 0x0A), (ZeroPage, 0x06), (ZeroPageX, 0x16), (Absolute, 0x0E), (AbsoluteX, 0x1E)]),
    ("bcc", &[(Relative, 0x90)]),
    ("bcs", &[(Relative, 0xB0)]),
    ("beq", &[(Relative, 0xF0)]),
    ("bmi", &[(Relative, 0x30)]),
    ("bne", &[(Relative, 0xD0)]),
    ("bpl", &[(Relative, 0x10)]),
    ("bvc", &[(Relative, 0x50)]),
    ("bvs", &[(Relative, 0x70)]),
    ("bit", &[(ZeroPage, 0x24), (Absolute, 0x2C)]),
    ("brk", &[(Implied, 0x00)]),
    ("clc", &[(Implied, 0x18)]),
    ("cld", &[(Implied, 0xD8)]),
    ("cli", &[(Implied, 0x58)]),
    ("clv", &[(Implied, 0xB8)]),
    ("cmp", &[(Immediate, 0xC9), (ZeroPage, 0xC5), (ZeroPageX, 0xD5), (Absolute, 0xCD), (AbsoluteX, 0xDD), (AbsoluteY, 0xD9), (IndirectX, 0xC1), (IndirectY, 0xD1)]),
    ("cpx", &[(Immediate, 0xE0), (ZeroPage, 0xE4), (Absolute, 0xEC)]),
    ("cpy", &[(Immediate, 0xC0), (ZeroPage, 0xC4), (Absolute, 0xCC)]),
    ("dec", &[(ZeroPage, 0xC6), (ZeroPageX, 0xD6), (Absolute, 0xCE), (AbsoluteX, 0xDE)]),
    ("dex", &[(Implied, 0xCA)]),
    ("dey", &[(Implied, 0x88)]),
    ("eor", &[(Immediate, 0x49), (ZeroPage, 0x45), (ZeroPageX, 0x55), (Absolute, 0x4D), (AbsoluteX, 0x5D), (AbsoluteY, 0x59), (IndirectX, 0x41), (IndirectY, 0x51)]),
    ("inc", &[(ZeroPage, 0xE6), (ZeroPageX, 0xF6), (Absolute, 0xEE), (AbsoluteX, 0xFE)]),
    ("inx", &[(Implied, 0xE8)]),
    ("iny", &[(Implied, 0xC8)]),
    ("jmp", &[(Absolute, 0x4C), (Indirect, 0x6C)]),
    ("jsr", &[(Absolute, 0x20)]),
    ("lda", &[(Immediate, 0xA9), (ZeroPage, 0xA5), (ZeroPageX, 0xB5), (Absolute, 0xAD), (AbsoluteX, 0xBD), (AbsoluteY, 0xB9), (IndirectX, 0xA1), (IndirectY, 0xB1)]),
    ("ldx", &[(Immediate, 0xA2), (ZeroPage, 0xA6), (ZeroPageY, 0xB6), (Absolute, 0xAE), (AbsoluteY, 0xBE)]),
    ("ldy", &[(Immediate, 0xA0), (ZeroPage, 0xA4), (ZeroPageX, 0xB4), (Absolute, 0xAC), (AbsoluteX, 0xBC)]),
    ("lsr", &[(Accumulator, 0x4A), (ZeroPage, 0x46), (ZeroPageX, 0x56), (Absolute, 0x4E), (AbsoluteX, 0x5E)]),
    ("nop", &[(Implied, 0xEA)]),
    ("ora", &[(Immediate, 0x09), (ZeroPage, 0x05), (ZeroPageX, 0x15), (Absolute, 0x0D), (AbsoluteX, 0x1D), (AbsoluteY, 0x19), (IndirectX, 0x01), (IndirectY, 0x11)]),
    ("pha", &[(Implied, 0x48)]),
    ("php", &[(Implied, 0x08)]),
    ("pla", &[(Implied, 0x68)]),
    ("plp", &[(Implied, 0x28)]),
    ("rol", &[(Accumulator, 0x2A), (ZeroPage, 0x26), (ZeroPageX, 0x36), (Absolute, 0x2E), (AbsoluteX, 0x3E)]),
    ("ror", &[(Accumulator, 0x6A), (ZeroPage, 0x66), (ZeroPageX, 0x76), (Absolute, 0x6E), (AbsoluteX, 0x7E)]),
    ("rti", &[(Implied, 0x40)]),
    ("rts", &[(Implied, 0x60)]),
    ("sbc", &[(Immediate, 0xE9), (ZeroPage, 0xE5), (ZeroPageX, 0xF5), (Absolute, 0xED), (AbsoluteX, 0xFD), (AbsoluteY, 0xF9), (IndirectX, 0xE1), (IndirectY, 0xF1)]),
    ("sec", &[(Implied, 0x38)]),
    ("sed", &[(Implied, 0xF8)]),
    ("sei", &[(Implied, 0x78)]),
    ("sta", &[(ZeroPage, 0x85), (ZeroPageX, 0x95), (Absolute, 0x8D), (AbsoluteX, 0x9D), (AbsoluteY, 0x99), (IndirectX, 0x81), (IndirectY, 0x91)]),
    ("stx", &[(ZeroPage, 0x86), (ZeroPageY, 0x96), (Absolute, 0x8E)]),
    ("sty", &[(ZeroPage, 0x84), (ZeroPageX, 0x94), (Absolute, 0x8C)]),
    ("tax", &[(Implied, 0xAA)]),
    ("tay", &[(Implied, 0xA8)]),
    ("tsx", &[(Implied, 0xBA)]),
    ("txa", &[(Implied, 0x8A)]),
    ("txs", &[(Implied, 0x9A)]),
    ("tya", &[(Implied, 0x98)]),
    // Undocumented, as the 6510 runs them.
    ("slo", &[(ZeroPage, 0x07), (ZeroPageX, 0x17), (Absolute, 0x0F), (AbsoluteX, 0x1F), (AbsoluteY, 0x1B), (IndirectX, 0x03), (IndirectY, 0x13)]),
    ("rla", &[(ZeroPage, 0x27), (ZeroPageX, 0x37), (Absolute, 0x2F), (AbsoluteX, 0x3F), (AbsoluteY, 0x3B), (IndirectX, 0x23), (IndirectY, 0x33)]),
    ("sre", &[(ZeroPage, 0x47), (ZeroPageX, 0x57), (Absolute, 0x4F), (AbsoluteX, 0x5F), (AbsoluteY, 0x5B), (IndirectX, 0x43), (IndirectY, 0x53)]),
    ("rra", &[(ZeroPage, 0x67), (ZeroPageX, 0x77), (Absolute, 0x6F), (AbsoluteX, 0x7F), (AbsoluteY, 0x7B), (IndirectX, 0x63), (IndirectY, 0x73)]),
    ("sax", &[(ZeroPage, 0x87), (ZeroPageY, 0x97), (Absolute, 0x8F), (IndirectX, 0x83)]),
    ("lax", &[(ZeroPage, 0xA7), (ZeroPageY, 0xB7), (Absolute, 0xAF), (AbsoluteY, 0xBF), (IndirectX, 0xA3), (IndirectY, 0xB3)]),
    ("dcp", &[(ZeroPage, 0xC7), (ZeroPageX, 0xD7), (Absolute, 0xCF), (AbsoluteX, 0xDF), (AbsoluteY, 0xDB), (IndirectX, 0xC3), (IndirectY, 0xD3)]),
    ("isc", &[(ZeroPage, 0xE7), (ZeroPageX, 0xF7), (Absolute, 0xEF), (AbsoluteX, 0xFF), (AbsoluteY, 0xFB), (IndirectX, 0xE3), (IndirectY, 0xF3)]),
    ("anc", &[(Immediate, 0x0B)]),
    ("alr", &[(Immediate, 0x4B)]),
    ("arr", &[(Immediate, 0x6B)]),
    ("sbx", &[(Immediate, 0xCB)]),
];

/// Other names some assemblers use for the undocumented ones.
fn canonical(mnemonic: &str) -> &str {
    match mnemonic {
        "isb" | "ins" => "isc",
        "asr" => "alr",
        "axs" => "sbx",
        "dcm" => "dcp",
        "lse" => "sre",
        "aso" => "slo",
        "axa" | "aax" => "sax",
        other => other,
    }
}

fn opcodes(mnemonic: &str) -> Option<&'static [(Mode, u8)]> {
    let mnemonic = canonical(mnemonic);
    OPCODES.iter().find(|(name, _)| *name == mnemonic).map(|(_, modes)| *modes)
}

fn opcode(modes: &[(Mode, u8)], mode: Mode) -> Option<u8> {
    modes.iter().find(|(m, _)| *m == mode).map(|(_, op)| *op)
}

impl Isa for M6502 {
    fn name(&self) -> &'static str {
        "6502"
    }

    fn is_instruction(&self, word: &str) -> bool {
        // ACME's +2 and the like after a mnemonic force a size: lda+2.
        opcodes(word.split('+').next().unwrap_or(word)).is_some()
    }

    fn colon_separates(&self) -> bool {
        true
    }

    fn local_prefixes(&self) -> &'static [char] {
        &['.', '@', '_']
    }

    fn cycles(&self, bytes: &[u8]) -> Option<String> {
        cycles(*bytes.first()?)
    }

    fn encode(&self, mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u8>, String> {
        // A size forced with +1 or +2 (ACME), or a: for absolute (ca65).
        let (mnemonic, forced) = match mnemonic.split_once('+') {
            Some((name, "1")) => (name, Some(1)),
            Some((name, "2")) => (name, Some(2)),
            _ => (mnemonic, None),
        };
        let modes = opcodes(mnemonic).ok_or_else(|| format!("{mnemonic} isn't a 6502 instruction"))?;
        let upper = mnemonic.to_ascii_uppercase();
        let text = operands.trim();
        let wrong = |what: &str| format!("{upper} can't take {what}");
        // No operand, or A.
        if text.is_empty() || text.eq_ignore_ascii_case("a") {
            if let Some(op) = opcode(modes, Accumulator).or_else(|| opcode(modes, Implied)) {
                return Ok(vec![op]);
            }
            return Err(format!("{upper} needs an operand"));
        }
        if let Some(op) = opcode(modes, Relative) {
            let target = line.eval(text)?;
            let offset = target - (line.here() + 2);
            if line.checking() && !(-128..=127).contains(&offset) {
                return Err(format!("{text} is {offset} bytes away, out of a branch's reach (-128 to 127); branch round a JMP"));
            }
            return Ok(vec![op, offset as u8]);
        }
        if let Some(value) = text.strip_prefix('#') {
            let op = opcode(modes, Immediate).ok_or_else(|| wrong("an immediate value"))?;
            let v = line.eval(value)?;
            if line.checking() && !(-128..=255).contains(&v) {
                return Err(format!("#{} is {v}, which doesn't fit in a byte", value.trim()));
            }
            return Ok(vec![op, v as u8]);
        }
        // (zp,X), (zp),Y and (abs).
        if let Some(inner) = text.strip_prefix('(') {
            let lower = inner.to_ascii_lowercase().replace(' ', "");
            if lower.ends_with(",x)") {
                let op = opcode(modes, IndirectX).ok_or_else(|| wrong("(address,X)"))?;
                let end = inner.rfind(',').unwrap_or(inner.len());
                return Ok(vec![op, zero_page(line, &inner[..end], &upper)?]);
            }
            if lower.ends_with("),y") {
                let op = opcode(modes, IndirectY).ok_or_else(|| wrong("(address),Y"))?;
                let end = inner.rfind(')').unwrap_or(inner.len());
                return Ok(vec![op, zero_page(line, &inner[..end], &upper)?]);
            }
            if lower.ends_with(')') && !lower[..lower.len() - 1].contains(')')
                && let Some(op) = opcode(modes, Indirect)
            {
                let end = inner.rfind(')').unwrap_or(inner.len());
                let v = line.eval(&inner[..end])?;
                return Ok(vec![op, v as u8, (v >> 8) as u8]);
            }
        }
        // address, address,X and address,Y: zero page if it's known to fit and the instruction has it.
        let (address, index) = match text.rsplit_once(',') {
            Some((address, index)) if index.trim().eq_ignore_ascii_case("x") => (address, Some('x')),
            Some((address, index)) if index.trim().eq_ignore_ascii_case("y") => (address, Some('y')),
            _ => (text, None),
        };
        let (address, forced) = match address.trim().split_once(':') {
            // ca65's z: and a: force zero page or absolute.
            Some((size, rest)) if size.eq_ignore_ascii_case("z") => (rest, Some(1)),
            Some((size, rest)) if size.eq_ignore_ascii_case("a") => (rest, Some(2)),
            _ => (address.trim(), forced),
        };
        let (zp_mode, abs_mode) = match index {
            Some('x') => (ZeroPageX, AbsoluteX),
            Some(_) => (ZeroPageY, AbsoluteY),
            None => (ZeroPage, Absolute),
        };
        let small = line.known(address).is_some_and(|v| (0..256).contains(&v));
        let zp = opcode(modes, zp_mode);
        let abs = opcode(modes, abs_mode);
        let v = line.eval(address)?;
        match (zp, abs, forced) {
            (Some(op), _, Some(1)) => Ok(vec![op, v as u8]),
            (_, Some(op), Some(2)) => Ok(vec![op, v as u8, (v >> 8) as u8]),
            (Some(op), _, _) if small => Ok(vec![op, v as u8]),
            (_, Some(op), _) => {
                if line.checking() && !(-32768..=65535).contains(&v) {
                    return Err(format!("{address} is {v}, which doesn't fit in 16 bits"));
                }
                Ok(vec![op, v as u8, (v >> 8) as u8])
            }
            (Some(op), None, _) => {
                if line.checking() && !(0..256).contains(&v) {
                    return Err(format!("{upper} {text} needs an address in zero page ($00 to $FF)"));
                }
                Ok(vec![op, v as u8])
            }
            (None, None, _) => Err(wrong(text)),
        }
    }
}

fn zero_page(line: &mut Line, text: &str, upper: &str) -> Result<u8, String> {
    let v = line.eval(text)?;
    if line.checking() && !(0..256).contains(&v) {
        return Err(format!("{upper}'s indirect address has to be in zero page ($00 to $FF), and {} is ${v:X}", text.trim()));
    }
    Ok(v as u8)
}

/// How long an instruction takes, in cycles: one more for crossing a page, or for a branch taken.
fn cycles(op: u8) -> Option<String> {
    let (name, mode) = OPCODES.iter().find_map(|(name, modes)| modes.iter().find(|(_, o)| *o == op).map(|(mode, _)| (*name, *mode)))?;
    let writes = matches!(name, "sta" | "stx" | "sty" | "sax");
    let changes = matches!(name, "asl" | "lsr" | "rol" | "ror" | "inc" | "dec" | "slo" | "rla" | "sre" | "rra" | "dcp" | "isc");
    let base: (u32, bool) = match (mode, changes, writes) {
        (Implied, ..) => match name {
            "brk" => (7, false),
            "rti" | "rts" => (6, false),
            "pha" | "php" => (3, false),
            "pla" | "plp" => (4, false),
            _ => (2, false),
        },
        (Accumulator | Immediate, ..) => (2, false),
        (Relative, ..) => return Some("2/3".into()),
        (ZeroPage, true, _) => (5, false),
        (ZeroPageX | ZeroPageY, true, _) => (6, false),
        (Absolute, true, _) => (6, false),
        (AbsoluteX | AbsoluteY, true, _) => (7, false),
        (IndirectX | IndirectY, true, _) => (8, false),
        (ZeroPage, ..) => (3, false),
        (ZeroPageX | ZeroPageY, ..) => (4, false),
        (Absolute, ..) if name == "jmp" => (3, false),
        (Absolute, ..) if name == "jsr" => (6, false),
        (Absolute, ..) => (4, false),
        (AbsoluteX | AbsoluteY, _, true) => (5, false),
        (AbsoluteX | AbsoluteY, ..) => (4, true),
        (Indirect, ..) => (5, false),
        (IndirectX, ..) => (6, false),
        (IndirectY, _, true) => (6, false),
        (IndirectY, ..) => (5, true),
    };
    Some(if base.1 { format!("{}/{}", base.0, base.0 + 1) } else { base.0.to_string() })
}
