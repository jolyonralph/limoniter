//! The 6502's later family: WDC's 65C02 (with the bit instructions Rockwell added, RMB, SMB, BBR and BBS, and
//! WDC's WAI and STP), and Hudson's HuC6280, the PC Engine's, a 65C02 with its own block moves, memory mapping
//! and video ports. Source says which with .SETCPU "65C02" (or "HuC6280"), .PC02, CPU 65C02 or PROCESSOR 65C02;
//! the 6502's own instructions are assembled as before, and its undocumented ones, which the later chips don't
//! have, aren't allowed.

use super::m6502::Mode;
use super::{Line, split_operands};
use crate::disasm::Decoded;

/// Which of the family.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Variant {
    /// The 6502 itself, with its undocumented instructions.
    #[default]
    Nmos,
    /// WDC's 65C02, Rockwell's instructions and all.
    C65,
    HuC6280,
}

impl Variant {
    pub fn name(self) -> &'static str {
        match self {
            Variant::Nmos => "6502",
            Variant::C65 => "65C02",
            Variant::HuC6280 => "HuC6280",
        }
    }

    /// The line that says so, at the top of a disassembly.
    pub fn directive(self) -> String {
        format!("        .setcpu \"{}\"", self.name())
    }
}

/// The setting kept from line to line: 0 for the 6502, 1 for the 65C02, 2 for the HuC6280.
pub const SETTING: &str = "6502 variant";

pub fn variant_of(line: &Line) -> Variant {
    match line.setting(SETTING) {
        Some(1) => Variant::C65,
        Some(2) => Variant::HuC6280,
        _ => Variant::Nmos,
    }
}

/// How an instruction of the later chips finds its operands, beyond the 6502's ways.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// One of the 6502's own ways.
    Base(Mode),
    /// (zp), without an index.
    ZeroPageIndirect,
    /// JMP (abs,X).
    AbsoluteIndexedIndirect,
    /// BBR and BBS: a byte of zero page, then where to branch.
    ZeroPageRelative,
    /// The HuC6280's block moves: from, to and how many, a word each.
    Block,
    /// The HuC6280's TST: a value, then where to test it.
    ImmediateZeroPage,
    ImmediateAbsolute,
    ImmediateZeroPageX,
    ImmediateAbsoluteX,
}

use Form::*;

/// The 65C02's, which the HuC6280 has as well.
const C02: &[(&str, &[(Form, u8)])] = &[
    ("adc", &[(ZeroPageIndirect, 0x72)]),
    ("and", &[(ZeroPageIndirect, 0x32)]),
    ("cmp", &[(ZeroPageIndirect, 0xD2)]),
    ("eor", &[(ZeroPageIndirect, 0x52)]),
    ("lda", &[(ZeroPageIndirect, 0xB2)]),
    ("ora", &[(ZeroPageIndirect, 0x12)]),
    ("sbc", &[(ZeroPageIndirect, 0xF2)]),
    ("sta", &[(ZeroPageIndirect, 0x92)]),
    ("bit", &[(Base(Mode::Immediate), 0x89), (Base(Mode::ZeroPageX), 0x34), (Base(Mode::AbsoluteX), 0x3C)]),
    ("dec", &[(Base(Mode::Accumulator), 0x3A)]),
    ("inc", &[(Base(Mode::Accumulator), 0x1A)]),
    ("jmp", &[(AbsoluteIndexedIndirect, 0x7C)]),
    ("bra", &[(Base(Mode::Relative), 0x80)]),
    ("phx", &[(Base(Mode::Implied), 0xDA)]),
    ("plx", &[(Base(Mode::Implied), 0xFA)]),
    ("phy", &[(Base(Mode::Implied), 0x5A)]),
    ("ply", &[(Base(Mode::Implied), 0x7A)]),
    ("stz", &[(Base(Mode::ZeroPage), 0x64), (Base(Mode::ZeroPageX), 0x74), (Base(Mode::Absolute), 0x9C), (Base(Mode::AbsoluteX), 0x9E)]),
    ("trb", &[(Base(Mode::ZeroPage), 0x14), (Base(Mode::Absolute), 0x1C)]),
    ("tsb", &[(Base(Mode::ZeroPage), 0x04), (Base(Mode::Absolute), 0x0C)]),
    ("rmb0", &[(Base(Mode::ZeroPage), 0x07)]),
    ("rmb1", &[(Base(Mode::ZeroPage), 0x17)]),
    ("rmb2", &[(Base(Mode::ZeroPage), 0x27)]),
    ("rmb3", &[(Base(Mode::ZeroPage), 0x37)]),
    ("rmb4", &[(Base(Mode::ZeroPage), 0x47)]),
    ("rmb5", &[(Base(Mode::ZeroPage), 0x57)]),
    ("rmb6", &[(Base(Mode::ZeroPage), 0x67)]),
    ("rmb7", &[(Base(Mode::ZeroPage), 0x77)]),
    ("smb0", &[(Base(Mode::ZeroPage), 0x87)]),
    ("smb1", &[(Base(Mode::ZeroPage), 0x97)]),
    ("smb2", &[(Base(Mode::ZeroPage), 0xA7)]),
    ("smb3", &[(Base(Mode::ZeroPage), 0xB7)]),
    ("smb4", &[(Base(Mode::ZeroPage), 0xC7)]),
    ("smb5", &[(Base(Mode::ZeroPage), 0xD7)]),
    ("smb6", &[(Base(Mode::ZeroPage), 0xE7)]),
    ("smb7", &[(Base(Mode::ZeroPage), 0xF7)]),
    ("bbr0", &[(ZeroPageRelative, 0x0F)]),
    ("bbr1", &[(ZeroPageRelative, 0x1F)]),
    ("bbr2", &[(ZeroPageRelative, 0x2F)]),
    ("bbr3", &[(ZeroPageRelative, 0x3F)]),
    ("bbr4", &[(ZeroPageRelative, 0x4F)]),
    ("bbr5", &[(ZeroPageRelative, 0x5F)]),
    ("bbr6", &[(ZeroPageRelative, 0x6F)]),
    ("bbr7", &[(ZeroPageRelative, 0x7F)]),
    ("bbs0", &[(ZeroPageRelative, 0x8F)]),
    ("bbs1", &[(ZeroPageRelative, 0x9F)]),
    ("bbs2", &[(ZeroPageRelative, 0xAF)]),
    ("bbs3", &[(ZeroPageRelative, 0xBF)]),
    ("bbs4", &[(ZeroPageRelative, 0xCF)]),
    ("bbs5", &[(ZeroPageRelative, 0xDF)]),
    ("bbs6", &[(ZeroPageRelative, 0xEF)]),
    ("bbs7", &[(ZeroPageRelative, 0xFF)]),
];

/// WDC's own, which the HuC6280 hasn't.
const WDC: &[(&str, &[(Form, u8)])] = &[("wai", &[(Base(Mode::Implied), 0xCB)]), ("stp", &[(Base(Mode::Implied), 0xDB)])];

/// The HuC6280's own.
const HUC: &[(&str, &[(Form, u8)])] = &[
    ("sxy", &[(Base(Mode::Implied), 0x02)]),
    ("sax", &[(Base(Mode::Implied), 0x22)]),
    ("say", &[(Base(Mode::Implied), 0x42)]),
    ("cla", &[(Base(Mode::Implied), 0x62)]),
    ("clx", &[(Base(Mode::Implied), 0x82)]),
    ("cly", &[(Base(Mode::Implied), 0xC2)]),
    ("st0", &[(Base(Mode::Immediate), 0x03)]),
    ("st1", &[(Base(Mode::Immediate), 0x13)]),
    ("st2", &[(Base(Mode::Immediate), 0x23)]),
    ("tma", &[(Base(Mode::Immediate), 0x43)]),
    ("tam", &[(Base(Mode::Immediate), 0x53)]),
    ("bsr", &[(Base(Mode::Relative), 0x44)]),
    ("csl", &[(Base(Mode::Implied), 0x54)]),
    ("csh", &[(Base(Mode::Implied), 0xD4)]),
    ("set", &[(Base(Mode::Implied), 0xF4)]),
    ("tii", &[(Block, 0x73)]),
    ("tdd", &[(Block, 0xC3)]),
    ("tin", &[(Block, 0xD3)]),
    ("tia", &[(Block, 0xE3)]),
    ("tai", &[(Block, 0xF3)]),
    ("tst", &[(ImmediateZeroPage, 0x83), (ImmediateAbsolute, 0x93), (ImmediateZeroPageX, 0xA3), (ImmediateAbsoluteX, 0xB3)]),
];

/// The 6502's undocumented instructions, which the later chips don't have.
const UNDOCUMENTED: [&str; 20] = ["slo", "rla", "sre", "rra", "sax", "lax", "dcp", "isc", "anc", "alr", "arr", "sbx", "lxa", "ane", "las", "tas", "sha", "shx", "shy", "jam"];

fn tables(variant: Variant) -> Vec<&'static [(&'static str, &'static [(Form, u8)])]> {
    match variant {
        Variant::Nmos => Vec::new(),
        Variant::C65 => vec![C02, WDC],
        Variant::HuC6280 => vec![HUC, C02],
    }
}

/// Whether a word is an instruction of any of the later chips (so that it isn't taken for a label).
pub fn is_instruction(word: &str) -> bool {
    [C02, WDC, HUC].iter().any(|t| t.iter().any(|(n, _)| *n == word))
}

/// Which chip has an instruction, if only a later one does.
pub fn which_has(word: &str) -> Option<Variant> {
    if C02.iter().chain(WDC).any(|(n, _)| *n == word) {
        Some(Variant::C65)
    } else if HUC.iter().any(|(n, _)| *n == word) {
        Some(Variant::HuC6280)
    } else {
        None
    }
}

/// An opcode, as a later chip runs it: its instruction and form.
pub fn decode(op: u8, variant: Variant) -> Option<(&'static str, Form)> {
    if variant == Variant::Nmos {
        return super::m6502::decode(op).map(|(n, m)| (n, Base(m)));
    }
    for table in tables(variant) {
        if let Some(found) = table.iter().find_map(|(name, forms)| forms.iter().find(|(_, o)| *o == op).map(|(f, _)| (*name, *f))) {
            return Some(found);
        }
    }
    // The 6502's own, but its undocumented ones (on the later chips, NOPs or their own).
    super::m6502::decode(op).filter(|(name, _)| !UNDOCUMENTED.contains(name) && (*name != "nop" || op == 0xEA)).map(|(n, m)| (n, Base(m)))
}

/// The directive that says which chip, if it's one: the variant, or None to leave it to the assembler.
pub fn said(word: &str, operands: &str) -> Option<Variant> {
    let word = word.trim_start_matches(['.', '!']);
    let value = operands.trim().trim_matches('"').to_ascii_lowercase();
    match word {
        "pc02" => Some(Variant::C65),
        "p02" => Some(Variant::Nmos),
        "setcpu" | "cpu" | "processor" => match value.as_str() {
            "65c02" | "w65c02" | "r65c02" | "wdc65c02" | "65sc02" => Some(Variant::C65),
            "huc6280" | "6280" | "huc" => Some(Variant::HuC6280),
            "6502" | "6510" | "6502x" | "6502i" | "nmos6502" => Some(Variant::Nmos),
            _ => None,
        },
        _ => None,
    }
}

/// An operand as written, for the later chips' own forms.
enum Written {
    None,
    Immediate(String),
    Address(String, Option<char>),
    Indirect(String),
    IndirectX(String),
    Two(String, String),
    Three(String, String, String),
    /// #value, address and maybe X, for TST.
    TestAt(String, String, bool),
}

fn written(text: &str) -> Written {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("a") {
        return Written::None;
    }
    let items = split_operands(text);
    if let Some(value) = items.first().and_then(|i| i.strip_prefix('#'))
        && items.len() >= 2
    {
        let x = items.get(2).is_some_and(|i| i.eq_ignore_ascii_case("x"));
        return Written::TestAt(value.trim().to_string(), items[1].clone(), x);
    }
    if let Some(value) = text.strip_prefix('#') {
        return Written::Immediate(value.trim().to_string());
    }
    if let Some(inner) = text.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        let lower = inner.to_ascii_lowercase().replace(' ', "");
        if let Some(address) = lower.strip_suffix(",x").map(|_| &inner[..inner.rfind(',').unwrap_or(inner.len())]) {
            return Written::IndirectX(address.trim().to_string());
        }
        if !inner.contains(['(', ')']) {
            return Written::Indirect(inner.trim().to_string());
        }
    }
    match items.len() {
        3 => Written::Three(items[0].clone(), items[1].clone(), items[2].clone()),
        2 if items[1].eq_ignore_ascii_case("x") => Written::Address(items[0].clone(), Some('x')),
        2 if items[1].eq_ignore_ascii_case("y") => Written::Address(items[0].clone(), Some('y')),
        2 => Written::Two(items[0].clone(), items[1].clone()),
        _ => Written::Address(text.to_string(), None),
    }
}

fn byte(line: &mut Line, text: &str, what: &str) -> Result<u8, String> {
    let v = line.eval(text)?;
    if line.checking() && !(-128..=255).contains(&v) {
        return Err(crate::i18n::tf!("{what} {} is {v}, which doesn't fit in a byte", text.trim()));
    }
    Ok(v as u8)
}

fn word(line: &mut Line, text: &str) -> Result<[u8; 2], String> {
    let v = line.eval(text)?;
    if line.checking() && !(-32768..=65535).contains(&v) {
        return Err(crate::i18n::tf!("{} is {v}, which doesn't fit in 16 bits", text.trim()));
    }
    Ok((v as u16).to_le_bytes())
}

/// An instruction of a later chip: Some with its bytes when it's one of that chip's own forms, None to leave it to
/// the 6502's own (which the later chips run the same).
pub fn encode(variant: Variant, mnemonic: &str, operands: &str, line: &mut Line) -> Option<Result<Vec<u8>, String>> {
    if variant == Variant::Nmos {
        return None;
    }
    let forms: Vec<(Form, u8)> = tables(variant).iter().flat_map(|t| t.iter().filter(|(n, _)| *n == mnemonic).flat_map(|(_, f)| f.iter().copied())).collect();
    if forms.is_empty() {
        return None;
    }
    let find = |form: Form| forms.iter().find(|(f, _)| *f == form).map(|(_, op)| *op);
    let upper = mnemonic.to_ascii_uppercase();
    let result = (|| -> Option<Result<Vec<u8>, String>> {
        Some(match written(operands) {
            Written::None => {
                let op = find(Base(Mode::Implied)).or_else(|| find(Base(Mode::Accumulator)))?;
                Ok(vec![op])
            }
            Written::Immediate(value) => {
                let op = find(Base(Mode::Immediate))?;
                byte(line, &value, "#").map(|b| vec![op, b])
            }
            Written::Indirect(address) => {
                let op = find(ZeroPageIndirect)?;
                byte(line, &address, "the zero page address").map(|b| vec![op, b])
            }
            Written::IndirectX(address) => {
                let op = find(AbsoluteIndexedIndirect)?;
                word(line, &address).map(|w| vec![op, w[0], w[1]])
            }
            Written::Two(zp, target) => {
                let op = find(ZeroPageRelative)?;
                (|| {
                    let zp = byte(line, &zp, "the zero page address")?;
                    let target = line.eval(&target)?;
                    let offset = target - (line.here() + 3);
                    if line.checking() && !(-128..=127).contains(&offset) {
                        return Err(crate::i18n::tf!("{upper}'s target is {offset} bytes away, out of a branch's reach (-128 to 127)"));
                    }
                    Ok(vec![op, zp, offset as u8])
                })()
            }
            Written::Three(from, to, length) => {
                let op = find(Block)?;
                (|| {
                    let mut bytes = vec![op];
                    for part in [from, to, length] {
                        bytes.extend(word(line, &part)?);
                    }
                    Ok(bytes)
                })()
            }
            Written::TestAt(value, address, x) => {
                let small = line.known(&address).is_some_and(|v| (0..256).contains(&v));
                let (zp, abs) = if x { (ImmediateZeroPageX, ImmediateAbsoluteX) } else { (ImmediateZeroPage, ImmediateAbsolute) };
                (|| {
                    let v = byte(line, &value, "#")?;
                    match (small, find(zp), find(abs)) {
                        (true, Some(op), _) => Ok(vec![op, v, byte(line, &address, "the address")?]),
                        (_, _, Some(op)) => word(line, &address).map(|w| vec![op, v, w[0], w[1]]),
                        _ => Err(crate::i18n::tf!("{upper} can't take {operands}")),
                    }
                })()
            }
            Written::Address(address, index) => {
                let (zp_mode, abs_mode, rel) = match index {
                    Some('x') => (Mode::ZeroPageX, Mode::AbsoluteX, None),
                    Some(_) => (Mode::ZeroPageY, Mode::AbsoluteY, None),
                    None => (Mode::ZeroPage, Mode::Absolute, find(Base(Mode::Relative))),
                };
                if let Some(op) = rel {
                    return Some((|| {
                        let target = line.eval(&address)?;
                        let offset = target - (line.here() + 2);
                        if line.checking() && !(-128..=127).contains(&offset) {
                            return Err(crate::i18n::tf!("{address} is {offset} bytes away, out of a branch's reach (-128 to 127); branch round a JMP"));
                        }
                        Ok(vec![op, offset as u8])
                    })());
                }
                let small = line.known(&address).is_some_and(|v| (0..256).contains(&v));
                match (find(Base(zp_mode)), find(Base(abs_mode))) {
                    (Some(op), _) if small || find(Base(abs_mode)).is_none() => byte(line, &address, "the zero page address").map(|b| vec![op, b]),
                    (_, Some(op)) => word(line, &address).map(|w| vec![op, w[0], w[1]]),
                    _ => return None,
                }
            }
        })
    })();
    match result {
        Some(done) => Some(done),
        // Its own instructions only have their own forms: say so rather than try the 6502's.
        None if which_has(mnemonic).is_some() && !super::m6502::has(mnemonic) => Some(Err(crate::i18n::tf!("{upper} can't take {}", operands.trim()))),
        None => None,
    }
}

/// Whether an instruction is the 6502's undocumented one, which the later chips haven't got.
pub fn undocumented(mnemonic: &str) -> bool {
    UNDOCUMENTED.contains(&mnemonic)
}

/// An instruction at `pc`, as a later chip runs it, for disassembling.
pub fn disassemble(at: &dyn Fn(usize) -> u8, pc: u16, variant: Variant) -> Decoded {
    let op = at(0);
    let Some((name, form)) = decode(op, variant) else {
        return Decoded { length: 1, text: format!("!byte ${op:02X}"), address: None, falls_through: true, target: None };
    };
    let byte = at(1);
    let hex = |b: u8| format!("${b:02X}");
    let nn = u16::from_le_bytes([at(1), at(2)]);
    let (length, operand, address) = match form {
        Base(Mode::Implied) => (1, String::new(), None),
        Base(Mode::Accumulator) => (1, " a".into(), None),
        Base(Mode::Immediate) => (2, format!(" #{}", hex(byte)), None),
        Base(Mode::ZeroPage) => (2, format!(" {}", hex(byte)), None),
        Base(Mode::ZeroPageX) => (2, format!(" {},x", hex(byte)), None),
        Base(Mode::ZeroPageY) => (2, format!(" {},y", hex(byte)), None),
        Base(Mode::Absolute) => (3, " {addr}".into(), Some(nn)),
        Base(Mode::AbsoluteX) => (3, " {addr},x".into(), Some(nn)),
        Base(Mode::AbsoluteY) => (3, " {addr},y".into(), Some(nn)),
        Base(Mode::Indirect) => (3, " ({addr})".into(), Some(nn)),
        Base(Mode::IndirectX) => (2, format!(" ({},x)", hex(byte)), None),
        Base(Mode::IndirectY) => (2, format!(" ({}),y", hex(byte)), None),
        Base(Mode::Relative) => (2, " {addr}".into(), Some(pc.wrapping_add(2).wrapping_add(byte as i8 as u16))),
        ZeroPageIndirect => (2, format!(" ({})", hex(byte)), None),
        AbsoluteIndexedIndirect => (3, " ({addr},x)".into(), Some(nn)),
        ZeroPageRelative => (3, format!(" {},{{addr}}", hex(byte)), Some(pc.wrapping_add(3).wrapping_add(at(2) as i8 as u16))),
        Block => (7, format!(" ${:04X},${:04X},${:04X}", nn, u16::from_le_bytes([at(3), at(4)]), u16::from_le_bytes([at(5), at(6)])), None),
        ImmediateZeroPage => (3, format!(" #{},{}", hex(byte), hex(at(2))), None),
        ImmediateZeroPageX => (3, format!(" #{},{},x", hex(byte), hex(at(2))), None),
        ImmediateAbsolute => (4, format!(" #{},{{addr}}", hex(byte)), Some(u16::from_le_bytes([at(2), at(3)]))),
        ImmediateAbsoluteX => (4, format!(" #{},{{addr}},x", hex(byte)), Some(u16::from_le_bytes([at(2), at(3)]))),
    };
    let branch = matches!(form, Base(Mode::Relative) | ZeroPageRelative);
    let (falls_through, target) = match name {
        "jmp" => (false, (form == Base(Mode::Absolute)).then_some(nn)),
        "bra" => (false, address),
        "rts" | "rti" | "brk" | "stp" => (false, None),
        "jsr" => (true, Some(nn)),
        _ if branch => (true, address),
        _ => (true, None),
    };
    Decoded { length, text: format!("{name}{operand}"), address, falls_through, target }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// MAME's opcode tables for the WDC 65C02 (dw65c02s.lst) and the HuC6280 (6280dasm.cpp), as an instruction
    /// and its form for each opcode, 00 to FF: the second word of each, as MAME names its modes.
    const MAME_65C02: &str = include_str!("testdata/m65c02_mame.txt");
    const MAME_HUC6280: &str = include_str!("testdata/huc6280_mame.txt");

    fn form_name(form: Form) -> &'static str {
        match form {
            Base(Mode::Implied) => "imp",
            Base(Mode::Accumulator) => "acc",
            Base(Mode::Immediate) => "imm",
            Base(Mode::ZeroPage) => "zpg",
            Base(Mode::ZeroPageX) => "zpx",
            Base(Mode::ZeroPageY) => "zpy",
            Base(Mode::Absolute) => "abs",
            Base(Mode::AbsoluteX) => "abx",
            Base(Mode::AbsoluteY) => "aby",
            Base(Mode::Indirect) => "ind",
            Base(Mode::IndirectX) => "idx",
            Base(Mode::IndirectY) => "idy",
            Base(Mode::Relative) => "rel",
            ZeroPageIndirect => "zpi",
            AbsoluteIndexedIndirect => "iax",
            ZeroPageRelative => "zrl",
            Block => "blk",
            ImmediateZeroPage => "imz",
            ImmediateAbsolute => "ima",
            ImmediateZeroPageX => "izx",
            ImmediateAbsoluteX => "imx",
        }
    }

    /// Sample operands for each form, and the bytes they make after the opcode, at $1000.
    fn sample(form: Form) -> (&'static str, Vec<u8>) {
        match form {
            Base(Mode::Implied) => ("", vec![]),
            Base(Mode::Accumulator) => ("a", vec![]),
            Base(Mode::Immediate) => ("#$12", vec![0x12]),
            Base(Mode::ZeroPage) => ("$12", vec![0x12]),
            Base(Mode::ZeroPageX) => ("$12,x", vec![0x12]),
            Base(Mode::ZeroPageY) => ("$12,y", vec![0x12]),
            Base(Mode::Absolute) => ("$1234", vec![0x34, 0x12]),
            Base(Mode::AbsoluteX) => ("$1234,x", vec![0x34, 0x12]),
            Base(Mode::AbsoluteY) => ("$1234,y", vec![0x34, 0x12]),
            Base(Mode::Indirect) => ("($1234)", vec![0x34, 0x12]),
            Base(Mode::IndirectX) => ("($12,x)", vec![0x12]),
            Base(Mode::IndirectY) => ("($12),y", vec![0x12]),
            Base(Mode::Relative) => ("$1010", vec![0x0E]),
            ZeroPageIndirect => ("($12)", vec![0x12]),
            AbsoluteIndexedIndirect => ("($1234,x)", vec![0x34, 0x12]),
            ZeroPageRelative => ("$12,$1010", vec![0x12, 0x0D]),
            Block => ("$1234,$5678,$0100", vec![0x34, 0x12, 0x78, 0x56, 0x00, 0x01]),
            ImmediateZeroPage => ("#$12,$34", vec![0x12, 0x34]),
            ImmediateAbsolute => ("#$12,$1234", vec![0x12, 0x34, 0x12]),
            ImmediateZeroPageX => ("#$12,$34,x", vec![0x12, 0x34]),
            ImmediateAbsoluteX => ("#$12,$1234,x", vec![0x12, 0x34, 0x12]),
        }
    }

    fn check(variant: Variant, mame: &str) {
        let table: Vec<(String, String)> = mame.lines().filter(|l| !l.starts_with('#')).map(|l| {
            let (name, mode) = l.split_once(' ').unwrap();
            (name.to_string(), mode.to_string())
        }).collect();
        assert_eq!(table.len(), 256);
        let mut instructions = 0;
        for (op, (name, mode)) in table.iter().enumerate() {
            let op = op as u8;
            let ours = decode(op, variant);
            // MAME's NOPs where the chips have nothing are left as data.
            if name == "ill" || (name == "nop" && op != 0xEA) {
                assert!(ours.is_none(), "{op:02X}: {ours:?}");
                continue;
            }
            let (our_name, form) = ours.unwrap_or_else(|| panic!("{op:02X}: {name} {mode}"));
            assert_eq!((our_name, form_name(form)), (name.as_str(), mode.as_str()), "{op:02X}");
            // Assembled from how it's written, it's this opcode; disassembled, it's written that way again.
            let (operand, after) = sample(form);
            let source = format!("  .setcpu \"{}\"\n  org $1000\n  {our_name} {operand}\n", variant.name());
            let assembly = crate::asm::assemble(&super::super::m6502::M6502, Path::new("t.a"), &source, &|_| Err("no".into()));
            assert!(assembly.ok(), "{source}: {:?}", assembly.problems);
            let mut expected = vec![op];
            expected.extend(after);
            assert_eq!(assembly.image(0x1000, assembly.highest().unwrap()), expected, "{source}");
            let decoded = disassemble(&|i| expected.get(i).copied().unwrap_or(0), 0x1000, variant);
            assert_eq!(decoded.length, expected.len(), "{op:02X}");
            let text = decoded.text.replace("{addr}", &decoded.address.map(|a| format!("${a:04X}")).unwrap_or_default());
            let wanted = if operand.is_empty() { our_name.to_string() } else { format!("{our_name} {operand}") };
            assert_eq!(text.to_ascii_lowercase(), wanted.to_ascii_lowercase(), "{op:02X}");
            instructions += 1;
        }
        assert!(instructions > 200, "{instructions}");
    }

    #[test]
    fn every_65c02_opcode_is_as_mame_has_it_both_ways() {
        check(Variant::C65, MAME_65C02);
    }

    #[test]
    fn every_huc6280_opcode_is_as_mame_has_it_both_ways() {
        check(Variant::HuC6280, MAME_HUC6280);
    }

    #[test]
    fn the_chip_is_chosen_by_the_source_and_its_own_are_kept_to_it() {
        let assemble = |source: &str| crate::asm::assemble(&super::super::m6502::M6502, Path::new("t.a"), source, &|_| Err("no".into()));
        // Without saying, BRA isn't the 6502's: the message says how to ask for it.
        let plain = assemble("  org $1000\n  bra *\n");
        assert!(plain.problems.iter().any(|p| p.message.contains(".setcpu")), "{:?}", plain.problems);
        for chip in ["  .setcpu \"65C02\"", "  .pc02", "  cpu 65c02", "  processor 65c02"] {
            let assembly = assemble(&format!("{chip}\n  org $1000\n  bra *\n  stz $12\n"));
            assert!(assembly.ok(), "{chip}: {:?}", assembly.problems);
            assert_eq!(assembly.image(0x1000, 0x1004), [0x80, 0xFE, 0x64, 0x12]);
        }
        // The 6502's undocumented ones aren't the 65C02's.
        let lax = assemble("  .setcpu \"65C02\"\n  lax $12\n");
        assert!(!lax.ok());
        // The HuC6280's SAX swaps A and X; on the 6502 it's the undocumented store.
        let huc = assemble("  .setcpu \"HuC6280\"\n  org $1000\n  sax\n  set\n  tii src,$2000,16\nsrc:\n");
        assert!(huc.ok(), "{:?}", huc.problems);
        assert_eq!(huc.image(0x1000, 0x1009), [0x22, 0xF4, 0x73, 0x09, 0x10, 0x00, 0x20, 0x10, 0x00]);
        // And SET is still the assembler's, where a label is given a value.
        let set = assemble("  .setcpu \"HuC6280\"\ncount set 3\n  org $1000\n  lda #count\n");
        assert!(set.ok(), "{:?}", set.problems);
        assert_eq!(set.image(0x1000, 0x1002), [0xA9, 3]);
    }
}
