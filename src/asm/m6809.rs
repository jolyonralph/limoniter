//! The Motorola 6809, as in the Dragon 32 and 64, the TRS-80 Color Computer and the Vectrex: every addressing mode,
//! indexed ones and all, as lwasm writes them. Direct page is used where an address is known to be in the page
//! SETDP names; < and > force direct or extended.
//!
//! And Hitachi's 6309, as lwasm takes it by default: registers E, F, W (E and F together), Q (D and W), V and MD;
//! arithmetic between registers (ADDR and the rest), TFM's block moves, the bit instructions (BAND ... STBT), AIM,
//! OIM, EIM and TIM on memory, MULD, DIVD and DIVQ, and indexing from W. PRAGMA 6809 takes the 6809's alone, and
//! PRAGMA 6309 the 6309's again.

use super::{Isa, Line, split_operands};

pub struct M6809;

/// An instruction that reads or writes memory: its page prefix, its immediate opcode (direct is 0x10 more,
/// indexed 0x20, extended 0x30), whether its values are 16 bits, and whether it has an immediate form.
pub(crate) struct Memory {
    pub name: &'static str,
    pub prefix: Option<u8>,
    pub opcode: u8,
    pub wide: bool,
    pub immediate: bool,
}

const fn m(name: &'static str, prefix: Option<u8>, opcode: u8, wide: bool, immediate: bool) -> Memory {
    Memory { name, prefix, opcode, wide, immediate }
}

pub(crate) const MEMORY: &[Memory] = &[
    m("suba", None, 0x80, false, true),
    m("cmpa", None, 0x81, false, true),
    m("sbca", None, 0x82, false, true),
    m("subd", None, 0x83, true, true),
    m("anda", None, 0x84, false, true),
    m("bita", None, 0x85, false, true),
    m("lda", None, 0x86, false, true),
    m("sta", None, 0x87, false, false),
    m("eora", None, 0x88, false, true),
    m("adca", None, 0x89, false, true),
    m("ora", None, 0x8A, false, true),
    m("adda", None, 0x8B, false, true),
    m("cmpx", None, 0x8C, true, true),
    m("jsr", None, 0x8D, true, false),
    m("ldx", None, 0x8E, true, true),
    m("stx", None, 0x8F, true, false),
    m("subb", None, 0xC0, false, true),
    m("cmpb", None, 0xC1, false, true),
    m("sbcb", None, 0xC2, false, true),
    m("addd", None, 0xC3, true, true),
    m("andb", None, 0xC4, false, true),
    m("bitb", None, 0xC5, false, true),
    m("ldb", None, 0xC6, false, true),
    m("stb", None, 0xC7, false, false),
    m("eorb", None, 0xC8, false, true),
    m("adcb", None, 0xC9, false, true),
    m("orb", None, 0xCA, false, true),
    m("addb", None, 0xCB, false, true),
    m("ldd", None, 0xCC, true, true),
    m("std", None, 0xCD, true, false),
    m("ldu", None, 0xCE, true, true),
    m("stu", None, 0xCF, true, false),
    m("cmpd", Some(0x10), 0x83, true, true),
    m("cmpy", Some(0x10), 0x8C, true, true),
    m("ldy", Some(0x10), 0x8E, true, true),
    m("sty", Some(0x10), 0x8F, true, false),
    m("lds", Some(0x10), 0xCE, true, true),
    m("sts", Some(0x10), 0xCF, true, false),
    m("cmpu", Some(0x11), 0x83, true, true),
    m("cmps", Some(0x11), 0x8C, true, true),
];

/// Instructions on memory alone (direct 0x00, indexed 0x60, extended 0x70), or on A (0x40) or B (0x50).
pub(crate) const SHIFTS: [(&str, u8); 12] = [("neg", 0x0), ("com", 0x3), ("lsr", 0x4), ("ror", 0x6), ("asr", 0x7), ("asl", 0x8), ("lsl", 0x8), ("rol", 0x9), ("dec", 0xA), ("inc", 0xC), ("tst", 0xD), ("clr", 0xF)];

pub(crate) const INHERENT: [(&str, &[u8]); 11] = [("nop", &[0x12]), ("sync", &[0x13]), ("daa", &[0x19]), ("sex", &[0x1D]), ("rts", &[0x39]), ("abx", &[0x3A]), ("rti", &[0x3B]), ("mul", &[0x3D]), ("swi", &[0x3F]), ("swi2", &[0x10, 0x3F]), ("swi3", &[0x11, 0x3F])];

/// Branches: the short opcode (the long one is 0x10 before it, except LBRA and LBSR).
pub(crate) const BRANCHES: [(&str, u8); 18] = [("bra", 0x20), ("brn", 0x21), ("bhi", 0x22), ("bls", 0x23), ("bcc", 0x24), ("bhs", 0x24), ("bcs", 0x25), ("blo", 0x25), ("bne", 0x26), ("beq", 0x27), ("bvc", 0x28), ("bvs", 0x29), ("bpl", 0x2A), ("bmi", 0x2B), ("bge", 0x2C), ("blt", 0x2D), ("bgt", 0x2E), ("ble", 0x2F)];

/// The 6309's own instructions on memory, as the 6809's are kept.
pub(crate) const MEMORY_6309: &[Memory] = &[
    m("subw", Some(0x10), 0x80, true, true),
    m("cmpw", Some(0x10), 0x81, true, true),
    m("sbcd", Some(0x10), 0x82, true, true),
    m("andd", Some(0x10), 0x84, true, true),
    m("bitd", Some(0x10), 0x85, true, true),
    m("ldw", Some(0x10), 0x86, true, true),
    m("stw", Some(0x10), 0x87, true, false),
    m("eord", Some(0x10), 0x88, true, true),
    m("adcd", Some(0x10), 0x89, true, true),
    m("ord", Some(0x10), 0x8A, true, true),
    m("addw", Some(0x10), 0x8B, true, true),
    // LDQ's immediate form is CD and four bytes, apart from these.
    m("ldq", Some(0x10), 0xCC, true, false),
    m("stq", Some(0x10), 0xCD, true, false),
    m("sube", Some(0x11), 0x80, false, true),
    m("cmpe", Some(0x11), 0x81, false, true),
    m("lde", Some(0x11), 0x86, false, true),
    m("ste", Some(0x11), 0x87, false, false),
    m("adde", Some(0x11), 0x8B, false, true),
    m("divd", Some(0x11), 0x8D, false, true),
    m("divq", Some(0x11), 0x8E, true, true),
    m("muld", Some(0x11), 0x8F, true, true),
    m("subf", Some(0x11), 0xC0, false, true),
    m("cmpf", Some(0x11), 0xC1, false, true),
    m("ldf", Some(0x11), 0xC6, false, true),
    m("stf", Some(0x11), 0xC7, false, false),
    m("addf", Some(0x11), 0xCB, false, true),
];

/// The 6309's instructions with no operand.
pub(crate) const INHERENT_6309: [(&str, &[u8]); 35] = [
    ("sexw", &[0x14]),
    ("pshsw", &[0x10, 0x38]),
    ("pulsw", &[0x10, 0x39]),
    ("pshuw", &[0x10, 0x3A]),
    ("puluw", &[0x10, 0x3B]),
    ("negd", &[0x10, 0x40]),
    ("comd", &[0x10, 0x43]),
    ("lsrd", &[0x10, 0x44]),
    ("rord", &[0x10, 0x46]),
    ("asrd", &[0x10, 0x47]),
    ("asld", &[0x10, 0x48]),
    ("lsld", &[0x10, 0x48]),
    ("rold", &[0x10, 0x49]),
    ("decd", &[0x10, 0x4A]),
    ("incd", &[0x10, 0x4C]),
    ("tstd", &[0x10, 0x4D]),
    ("clrd", &[0x10, 0x4F]),
    ("comw", &[0x10, 0x53]),
    ("lsrw", &[0x10, 0x54]),
    ("rorw", &[0x10, 0x56]),
    ("rolw", &[0x10, 0x59]),
    ("decw", &[0x10, 0x5A]),
    ("incw", &[0x10, 0x5C]),
    ("tstw", &[0x10, 0x5D]),
    ("clrw", &[0x10, 0x5F]),
    ("come", &[0x11, 0x43]),
    ("dece", &[0x11, 0x4A]),
    ("ince", &[0x11, 0x4C]),
    ("tste", &[0x11, 0x4D]),
    ("clre", &[0x11, 0x4F]),
    ("comf", &[0x11, 0x53]),
    ("decf", &[0x11, 0x5A]),
    ("incf", &[0x11, 0x5C]),
    ("tstf", &[0x11, 0x5D]),
    ("clrf", &[0x11, 0x5F]),
];

/// Arithmetic between two registers: the second byte, after 0x10.
pub(crate) const REGISTER_OPS: [(&str, u8); 8] = [("addr", 0x30), ("adcr", 0x31), ("subr", 0x32), ("sbcr", 0x33), ("andr", 0x34), ("orr", 0x35), ("eorr", 0x36), ("cmpr", 0x37)];

/// Bit instructions between a register's bit and one in the direct page: the second byte, after 0x11.
pub(crate) const BIT_OPS: [(&str, u8); 8] = [("band", 0x30), ("biand", 0x31), ("bor", 0x32), ("bior", 0x33), ("beor", 0x34), ("bieor", 0x35), ("ldbt", 0x36), ("stbt", 0x37)];

/// An immediate value with memory: the direct opcode (indexed is 0x60 more, extended 0x70).
pub(crate) const IMMEDIATE_MEMORY: [(&str, u8); 4] = [("oim", 0x01), ("aim", 0x02), ("eim", 0x05), ("tim", 0x0B)];

fn memory(name: &str) -> Option<&'static Memory> {
    MEMORY.iter().chain(MEMORY_6309).find(|m| m.name == name)
}

/// Whether a word is one of the 6309's own instructions.
fn only_6309(word: &str) -> bool {
    MEMORY_6309.iter().any(|m| m.name == word)
        || INHERENT_6309.iter().any(|(n, _)| *n == word)
        || REGISTER_OPS.iter().any(|(n, _)| *n == word)
        || BIT_OPS.iter().any(|(n, _)| *n == word)
        || IMMEDIATE_MEMORY.iter().any(|(n, _)| *n == word)
        || matches!(word, "tfm" | "ldmd" | "bitmd")
}

/// Whether the 6309's instructions are taken here: unless PRAGMA 6809 said not.
fn with_6309(line: &Line) -> bool {
    line.setting("6309").is_none_or(|on| on != 0)
}

fn shift(name: &str) -> Option<(u8, Option<char>)> {
    // NEGA, CLRB and so on work on a register; NEG and CLR on memory.
    if let Some((_, op)) = SHIFTS.iter().find(|(n, _)| *n == name) {
        return Some((*op, None));
    }
    let (stem, register) = name.split_at(name.len().checked_sub(1)?);
    let register = register.chars().next()?;
    if register != 'a' && register != 'b' {
        return None;
    }
    SHIFTS.iter().find(|(n, _)| *n == stem).map(|(_, op)| (*op, Some(register)))
}

fn branch(name: &str) -> Option<(bool, u8)> {
    match name {
        "bsr" => return Some((false, 0x8D)),
        "lbsr" => return Some((true, 0x17)),
        "lbra" => return Some((true, 0x16)),
        _ => {}
    }
    if let Some(short) = name.strip_prefix('l')
        && let Some((_, op)) = BRANCHES.iter().find(|(n, _)| *n == short)
    {
        return Some((true, *op));
    }
    BRANCHES.iter().find(|(n, _)| *n == name).map(|(_, op)| (false, *op))
}

fn register_code(name: &str, h6309: bool) -> Option<u8> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "d" => 0,
        "x" => 1,
        "y" => 2,
        "u" => 3,
        "s" => 4,
        "pc" | "pcr" => 5,
        "a" => 8,
        "b" => 9,
        "cc" | "ccr" => 10,
        "dp" => 11,
        "w" if h6309 => 6,
        "v" if h6309 => 7,
        "0" | "z" if h6309 => 12,
        "e" if h6309 => 14,
        "f" if h6309 => 15,
        _ => return None,
    })
}

fn stack_bit(name: &str, user: bool) -> Option<u8> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "pc" => 0x80,
        "u" if !user => 0x40,
        "s" if user => 0x40,
        "y" => 0x20,
        "x" => 0x10,
        "dp" => 0x08,
        "b" => 0x04,
        "a" => 0x02,
        "d" => 0x06,
        "cc" | "ccr" => 0x01,
        "all" => 0xFF,
        _ => return None,
    })
}

fn index_register(name: &str) -> Option<u8> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "x" => 0,
        "y" => 1,
        "u" => 2,
        "s" => 3,
        _ => return None,
    })
}

impl Isa for M6809 {
    fn name(&self) -> &'static str {
        "6809"
    }

    fn big_endian(&self) -> bool {
        true
    }

    fn operand_ends_at_space(&self) -> bool {
        true
    }

    fn is_instruction(&self, word: &str) -> bool {
        memory(word).is_some() || shift(word).is_some() || branch(word).is_some() || INHERENT.iter().any(|(n, _)| *n == word) || only_6309(word) || matches!(word, "jmp" | "leax" | "leay" | "leas" | "leau" | "pshs" | "puls" | "pshu" | "pulu" | "tfr" | "exg" | "orcc" | "andcc" | "cwai")
    }

    fn directive(&self, word: &str, operands: &str, line: &mut Line) -> Option<Result<Vec<u8>, String>> {
        // PRAGMA 6809 and PRAGMA 6309 (among lwasm's others, which change nothing here).
        if word != "pragma" {
            return None;
        }
        for pragma in split_operands(operands) {
            match pragma.trim().to_ascii_lowercase().as_str() {
                "6809" => line.set_setting("6309", 0),
                "6309" => line.set_setting("6309", 1),
                _ => {}
            }
        }
        Some(Ok(Vec::new()))
    }

    fn is_directive(&self, word: &str) -> bool {
        word == "pragma"
    }

    /// Its cycles on a 6809 (or a 6309 in emulation mode), and in the 6309's native mode where that's different.
    fn cycles(&self, bytes: &[u8]) -> Option<String> {
        crate::m6809_timing::both(bytes)
    }

    fn encode(&self, mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u8>, String> {
        let upper = mnemonic.to_ascii_uppercase();
        let h6309 = with_6309(line);
        if only_6309(mnemonic) {
            if !h6309 {
                return Err(crate::i18n::tf!("{upper} is the 6309's, not the 6809's; PRAGMA 6309 takes the 6309's instructions"));
            }
            if let Some(bytes) = encode_6309(mnemonic, operands, line)? {
                return Ok(bytes);
            }
        }
        if let Some((_, bytes)) = INHERENT.iter().find(|(n, _)| *n == mnemonic) {
            return Ok(bytes.to_vec());
        }
        if let Some((long, op)) = branch(mnemonic) {
            let target = line.eval(operands)?;
            let length: i64 = match (long, op) {
                (false, _) => 2,
                (true, 0x16 | 0x17) => 3,
                (true, _) => 4,
            };
            let offset = target - (line.here() + length);
            if !long {
                if line.checking() && !(-128..=127).contains(&offset) {
                    return Err(crate::i18n::tf!("{operands} is {offset} bytes away, out of a short branch's reach; use L{upper}"));
                }
                return Ok(vec![op, offset as u8]);
            }
            let [high, low] = (offset as u16).to_be_bytes();
            return Ok(if length == 3 { vec![op, high, low] } else { vec![0x10, op, high, low] });
        }
        match mnemonic {
            "orcc" | "andcc" | "cwai" => {
                let value = operands.strip_prefix('#').ok_or_else(|| crate::i18n::tf!("{upper} takes # and a value"))?;
                let op = match mnemonic {
                    "orcc" => 0x1A,
                    "andcc" => 0x1C,
                    _ => 0x3C,
                };
                return Ok(vec![op, byte(line, value)?]);
            }
            "tfr" | "exg" => {
                let items = split_operands(operands);
                let [from, to] = items.as_slice() else {
                    return Err(crate::i18n::tf!("{upper} takes two registers"));
                };
                let (Some(a), Some(b)) = (register_code(from, h6309), register_code(to, h6309)) else {
                    return Err(crate::i18n::tf!("{upper} takes two registers, as {upper} A,B"));
                };
                return Ok(vec![if mnemonic == "tfr" { 0x1F } else { 0x1E }, a << 4 | b]);
            }
            "pshs" | "puls" | "pshu" | "pulu" => {
                let user = mnemonic.ends_with('u');
                let mut mask = 0u8;
                for name in split_operands(operands) {
                    mask |= stack_bit(&name, user).ok_or_else(|| crate::i18n::tf!("{name} can't go on {} stack", if user { "the U" } else { "the S" }))?;
                }
                let op = match mnemonic {
                    "pshs" => 0x34,
                    "puls" => 0x35,
                    "pshu" => 0x36,
                    _ => 0x37,
                };
                return Ok(vec![op, mask]);
            }
            "leax" | "leay" | "leas" | "leau" => {
                let op = 0x30 + ["leax", "leay", "leas", "leau"].iter().position(|n| *n == mnemonic).unwrap_or(0) as u8;
                let mut bytes = vec![op];
                bytes.extend(indexed(line, operands, 1, h6309)?.ok_or_else(|| crate::i18n::tf!("{upper} takes an indexed address, as {upper} 2,X"))?);
                return Ok(bytes);
            }
            _ => {}
        }
        if let Some((op, register)) = shift(mnemonic) {
            if let Some(register) = register {
                return Ok(vec![op | if register == 'a' { 0x40 } else { 0x50 }]);
            }
            return address(line, operands, None, [op, op | 0x60, op | 0x70], 1, h6309);
        }
        if mnemonic == "jmp" {
            return address(line, operands, None, [0x0E, 0x6E, 0x7E], 1, h6309);
        }
        let found = memory(mnemonic).ok_or_else(|| crate::i18n::tf!("{mnemonic} isn't a 6809 instruction"))?;
        let mut head = Vec::new();
        head.extend(found.prefix);
        if let Some(value) = operands.strip_prefix('#') {
            if !found.immediate {
                return Err(crate::i18n::tf!("{upper} can't take an immediate value"));
            }
            head.push(found.opcode);
            if found.wide {
                head.extend((line.eval(value)? as u16).to_be_bytes());
            } else {
                head.push(byte(line, value)?);
            }
            return Ok(head);
        }
        address(line, operands, found.prefix, [found.opcode + 0x10, found.opcode + 0x20, found.opcode + 0x30], head.len() + 1, h6309)
    }
}

/// The 6309's instructions that aren't in its tables of memory and inherent ones; None for those that are.
fn encode_6309(mnemonic: &str, operands: &str, line: &mut Line) -> Result<Option<Vec<u8>>, String> {
    let upper = mnemonic.to_ascii_uppercase();
    if let Some((_, bytes)) = INHERENT_6309.iter().find(|(n, _)| *n == mnemonic) {
        return Ok(Some(bytes.to_vec()));
    }
    let items = split_operands(operands);
    if let Some((_, op)) = REGISTER_OPS.iter().find(|(n, _)| *n == mnemonic) {
        let [from, to] = items.as_slice() else {
            return Err(crate::i18n::tf!("{upper} takes two registers, as {upper} A,B"));
        };
        let (Some(a), Some(b)) = (register_code(from, true), register_code(to, true)) else {
            return Err(crate::i18n::tf!("{upper} takes two registers, as {upper} A,B"));
        };
        return Ok(Some(vec![0x10, *op, a << 4 | b]));
    }
    if mnemonic == "tfm" {
        // R+,R+  R-,R-  R+,R  R,R+: which way each register goes.
        let [from, to] = items.as_slice() else {
            return Err(crate::i18n::tr("TFM takes two registers, as TFM X+,Y+").into());
        };
        let split = |text: &str| -> (String, char) {
            let text = text.trim().to_ascii_lowercase();
            match text.strip_suffix('+').map(|t| (t.to_string(), '+')).or_else(|| text.strip_suffix('-').map(|t| (t.to_string(), '-'))) {
                Some(found) => found,
                None => (text, ' '),
            }
        };
        let ((a, way_a), (b, way_b)) = (split(from), split(to));
        let op = match (way_a, way_b) {
            ('+', '+') => 0x38,
            ('-', '-') => 0x39,
            ('+', ' ') => 0x3A,
            (' ', '+') => 0x3B,
            _ => return Err(crate::i18n::tr("TFM moves R+,R+  R-,R-  R+,R  or R,R+").into()),
        };
        let (Some(a), Some(b)) = (register_code(&a, true).filter(|c| *c <= 4), register_code(&b, true).filter(|c| *c <= 4)) else {
            return Err(crate::i18n::tr("TFM moves between D, X, Y, U and S").into());
        };
        return Ok(Some(vec![0x11, op, a << 4 | b]));
    }
    if let Some((_, op)) = BIT_OPS.iter().find(|(n, _)| *n == mnemonic) {
        // r,source bit,destination bit,address; or lwasm's r.bit,address.bit.
        let parts: Vec<String> = if items.len() == 2 {
            items.iter().flat_map(|i| i.rsplitn(2, '.').collect::<Vec<_>>().into_iter().rev().map(str::to_string).collect::<Vec<_>>()).collect()
        } else {
            items.clone()
        };
        let [register, source, destination, address] = parts.as_slice() else {
            return Err(crate::i18n::tf!("{upper} takes a register, two bit numbers and an address, as {upper} A,0,7,$40"));
        };
        let register = match register.trim().to_ascii_lowercase().as_str() {
            "cc" | "ccr" => 0,
            "a" => 1,
            "b" => 2,
            _ => return Err(crate::i18n::tf!("{upper} works on CC, A or B")),
        };
        let bit = |line: &mut Line, text: &str| -> Result<u8, String> {
            let value = line.eval(text)?;
            if line.checking() && !(0..=7).contains(&value) {
                return Err(crate::i18n::tf!("a bit is 0 to 7, not {value}"));
            }
            Ok(value as u8 & 7)
        };
        let (s, d) = (bit(line, source)?, bit(line, destination)?);
        let address = address.trim().trim_start_matches('<');
        let value = line.eval(address)?;
        return Ok(Some(vec![0x11, *op, register << 6 | s << 3 | d, value as u8]));
    }
    if let Some((_, op)) = IMMEDIATE_MEMORY.iter().find(|(n, _)| *n == mnemonic) {
        // #value,address (or lwasm's #value;address).
        let (value, place) = match operands.split_once(';') {
            Some((v, p)) => (v.to_string(), p.to_string()),
            None => match operands.split_once(',') {
                Some((v, p)) => (v.to_string(), p.to_string()),
                None => return Err(crate::i18n::tf!("{upper} takes # and a value, then an address, as {upper} #$0F,$40")),
            },
        };
        let value = value.trim().strip_prefix('#').ok_or_else(|| crate::i18n::tf!("{upper} takes # and a value, then an address, as {upper} #$0F,$40"))?;
        let value = byte(line, value)?;
        let mut bytes = address(line, &place, None, [*op, op | 0x60, op | 0x70], 2, true)?;
        bytes.insert(1, value);
        return Ok(Some(bytes));
    }
    match mnemonic {
        "ldmd" | "bitmd" => {
            let value = operands.trim().strip_prefix('#').ok_or_else(|| crate::i18n::tf!("{upper} takes # and a value"))?;
            Ok(Some(vec![0x11, if mnemonic == "ldmd" { 0x3D } else { 0x3C }, byte(line, value)?]))
        }
        "ldq" if operands.trim().starts_with('#') => {
            let value = line.eval(&operands.trim()[1..])?;
            let mut bytes = vec![0xCD];
            bytes.extend((value as u32).to_be_bytes());
            Ok(Some(bytes))
        }
        _ => Ok(None),
    }
}

fn byte(line: &mut Line, text: &str) -> Result<u8, String> {
    let value = line.eval(text)?;
    if line.checking() && !(-128..=255).contains(&value) {
        return Err(crate::i18n::tf!("{} is {value}, which doesn't fit in a byte", text.trim()));
    }
    Ok(value as u8)
}

/// An address: indexed, direct or extended, with the opcodes for each. `before` is how many bytes come before
/// the postbyte, for offsets from PC.
fn address(line: &mut Line, operands: &str, prefix: Option<u8>, [direct, indexed_op, extended]: [u8; 3], before: usize, h6309: bool) -> Result<Vec<u8>, String> {
    let mut bytes: Vec<u8> = prefix.into_iter().collect();
    if let Some(post) = indexed(line, operands, before, h6309)? {
        bytes.push(indexed_op);
        bytes.extend(post);
        return Ok(bytes);
    }
    let (text, forced) = match operands.trim() {
        t if t.starts_with('<') => (&t[1..], Some(true)),
        t if t.starts_with('>') => (&t[1..], Some(false)),
        t => (t, None),
    };
    let page = line.direct_page();
    let in_page = line.known(text).zip(page).is_some_and(|(v, dp)| (v >> 8) & 0xFF == dp && (0..=0xFFFF).contains(&v));
    let value = line.eval(text)?;
    if forced.unwrap_or(in_page) {
        bytes.push(direct);
        bytes.push(value as u8);
    } else {
        bytes.push(extended);
        bytes.extend((value as u16).to_be_bytes());
    }
    Ok(bytes)
}

/// The postbyte and offset of an indexed address (n,X  ,X+  A,Y  [n,PCR] ...), or None if it isn't one.
fn indexed(line: &mut Line, operands: &str, before: usize, h6309: bool) -> Result<Option<Vec<u8>>, String> {
    let text = operands.trim();
    let (text, indirect) = match text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        Some(inner) => (inner.trim(), true),
        None => (text, false),
    };
    let ind = if indirect { 0x10 } else { 0 };
    let Some((offset, register)) = text.rsplit_once(',') else {
        // [address]: extended indirect.
        if indirect {
            let value = line.eval(text)?;
            let mut bytes = vec![0x9F];
            bytes.extend((value as u16).to_be_bytes());
            return Ok(Some(bytes));
        }
        return Ok(None);
    };
    let register = register.trim();
    let offset = offset.trim();
    let lower = register.to_ascii_lowercase();
    // Auto increment and decrement.
    let (name, post) = if let Some(r) = lower.strip_suffix("++") {
        (r.to_string(), Some(0x81))
    } else if let Some(r) = lower.strip_suffix('+') {
        (r.to_string(), Some(0x80))
    } else if let Some(r) = lower.strip_prefix("--") {
        (r.to_string(), Some(0x83))
    } else if let Some(r) = lower.strip_prefix('-') {
        (r.to_string(), Some(0x82))
    } else {
        (lower.clone(), None)
    };
    if name == "pc" || name == "pcr" {
        // From the end of the instruction: the offset is to the label (PCR), or as written (PC).
        let value = line.eval(if offset.is_empty() { "0" } else { offset })?;
        let small = line.known(offset).is_some();
        let short_offset = if name == "pcr" { value - (line.here() + before as i64 + 2) } else { value };
        if small && (-128..=127).contains(&short_offset) {
            return Ok(Some(vec![0x8C | ind, short_offset as u8]));
        }
        let long_offset = if name == "pcr" { value - (line.here() + before as i64 + 3) } else { value };
        let mut bytes = vec![0x8D | ind];
        bytes.extend((long_offset as u16).to_be_bytes());
        return Ok(Some(bytes));
    }
    // The 6309's W: ,W  n,W (16 bits)  ,W++  ,--W, and each of them indirect.
    if h6309 && name == "w" {
        let post = match post {
            None if offset.is_empty() => 0x8F,
            None => 0xAF,
            Some(0x81) => 0xCF,
            Some(0x83) => 0xEF,
            Some(_) => return Err(crate::i18n::tr("W can be indexed from as ,W  n,W  ,W++ or ,--W").into()),
        };
        // Indirect, they're 0x90, 0xB0, 0xD0 and 0xF0.
        let post = if indirect { (post & 0xE0) | 0x10 } else { post };
        let mut bytes = vec![post];
        if post & 0x60 == 0x20 {
            // Always 16 bits from W, so a < or > for the size says nothing.
            let value = line.eval(offset.trim_start_matches(['<', '>']))?;
            bytes.extend((value as u16).to_be_bytes());
        }
        return Ok(Some(bytes));
    }
    let Some(r) = index_register(&name) else {
        return Err(crate::i18n::tf!("{register} can't be indexed from; X, Y, U, S or PC can"));
    };
    let rr = r << 5;
    if let Some(post) = post {
        if !offset.is_empty() {
            return Err(crate::i18n::tr("an offset can't go with auto increment or decrement").into());
        }
        if indirect && (post == 0x80 || post == 0x82) {
            return Err(crate::i18n::tr("only ,R++ and ,--R can be indirect").into());
        }
        return Ok(Some(vec![0x80 | rr | post | ind]));
    }
    match offset.to_ascii_lowercase().as_str() {
        "" => return Ok(Some(vec![0x84 | rr | ind])),
        "a" => return Ok(Some(vec![0x86 | rr | ind])),
        "b" => return Ok(Some(vec![0x85 | rr | ind])),
        "d" => return Ok(Some(vec![0x8B | rr | ind])),
        "e" if h6309 => return Ok(Some(vec![0x87 | rr | ind])),
        "f" if h6309 => return Ok(Some(vec![0x8A | rr | ind])),
        "w" if h6309 => return Ok(Some(vec![0x8E | rr | ind])),
        _ => {}
    }
    // A size forced with < or >, or the smallest that fits once the value's known.
    let (offset, forced) = match offset {
        o if o.starts_with("<<") => (&o[2..], Some(5)),
        o if o.starts_with('<') => (&o[1..], Some(8)),
        o if o.starts_with('>') => (&o[1..], Some(16)),
        o => (o, None),
    };
    let known = line.known(offset);
    let value = line.eval(offset)?;
    let size = forced.unwrap_or(match known {
        Some(0) => 0,
        Some(v) if (-16..=15).contains(&v) && !indirect => 5,
        Some(v) if (-128..=127).contains(&v) => 8,
        _ => 16,
    });
    Ok(Some(match size {
        0 => vec![0x84 | rr | ind],
        5 => vec![rr | (value as u8 & 0x1F)],
        8 => vec![0x88 | rr | ind, value as u8],
        _ => {
            let mut bytes = vec![0x89 | rr | ind];
            bytes.extend((value as u16).to_be_bytes());
            bytes
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assemble(source: &str) -> crate::asm::Assembly {
        crate::asm::assemble(&M6809, std::path::Path::new("t.asm"), source, &|_| Err("no".into()))
    }

    /// Each line's bytes, from the 6309's opcode map as Hitachi's manual and lwasm have them.
    #[test]
    fn the_6309s_instructions_assemble_as_lwasm_assembles_them() {
        let cases: &[(&str, &[u8])] = &[
            ("ldq #$12345678", &[0xCD, 0x12, 0x34, 0x56, 0x78]),
            ("ldq $1234", &[0x10, 0xFC, 0x12, 0x34]),
            ("stq ,x", &[0x10, 0xED, 0x84]),
            ("addr a,b", &[0x10, 0x30, 0x89]),
            ("cmpr x,y", &[0x10, 0x37, 0x12]),
            ("tfm x+,y+", &[0x11, 0x38, 0x12]),
            ("tfm u-,s-", &[0x11, 0x39, 0x34]),
            ("tfm x+,d", &[0x11, 0x3A, 0x10]),
            ("tfm d,x+", &[0x11, 0x3B, 0x01]),
            ("band a,0,7,$40", &[0x11, 0x30, 0x47, 0x40]),
            ("stbt cc,3,1,$80", &[0x11, 0x37, 0x19, 0x80]),
            ("aim #$0F,<$40", &[0x02, 0x0F, 0x40]),
            ("oim #$80,2,x", &[0x61, 0x80, 0x02]),
            ("tim #1,>$1234", &[0x7B, 0x01, 0x12, 0x34]),
            ("ldw #$1234", &[0x10, 0x86, 0x12, 0x34]),
            ("addw <$20", &[0x10, 0x9B, 0x20]),
            ("lde #5", &[0x11, 0x86, 0x05]),
            ("stf $2000", &[0x11, 0xF7, 0x20, 0x00]),
            ("muld #$100", &[0x11, 0x8F, 0x01, 0x00]),
            ("divd #3", &[0x11, 0x8D, 0x03]),
            ("divq #$100", &[0x11, 0x8E, 0x01, 0x00]),
            ("lda ,w", &[0xA6, 0x8F]),
            ("lda 300,w", &[0xA6, 0xAF, 0x01, 0x2C]),
            ("ldd [,w++]", &[0xEC, 0xD0]),
            ("ldd [,--w]", &[0xEC, 0xF0]),
            ("leax e,x", &[0x30, 0x87]),
            ("leay f,y", &[0x31, 0xAA]),
            ("ldb w,u", &[0xE6, 0xCE]),
            ("pshsw", &[0x10, 0x38]),
            ("clrd", &[0x10, 0x4F]),
            ("incw", &[0x10, 0x5C]),
            ("tste", &[0x11, 0x4D]),
            ("sexw", &[0x14]),
            ("tfr w,v", &[0x1F, 0x67]),
            ("tfr 0,a", &[0x1F, 0xC8]),
            ("ldmd #1", &[0x11, 0x3D, 0x01]),
            ("bitmd #$80", &[0x11, 0x3C, 0x80]),
        ];
        for (source, bytes) in cases {
            let assembly = assemble(&format!("\torg $3000\n\t{source}\n"));
            assert!(assembly.problems.is_empty(), "{source}: {:?}", assembly.problems);
            let made: Vec<u8> = assembly.lines.iter().flat_map(|l| l.bytes.clone()).collect();
            assert_eq!(made, *bytes, "{source}");
            // And back: disassembled as the 6309, the same bytes.
            let decoded = crate::disasm::m6809(&|i| bytes.get(i).copied().unwrap_or(0), 0x3000, true);
            assert_eq!(decoded.length, bytes.len(), "{source} came back as {}", decoded.text);
            let again = assemble(&format!("\torg $3000\n\t{}\n", decoded.text.replace("{addr}", "$1234")));
            let remade: Vec<u8> = again.lines.iter().flat_map(|l| l.bytes.clone()).collect();
            if !decoded.text.contains("{addr}") {
                assert_eq!(remade, *bytes, "{source} -> {}", decoded.text);
            }
        }
    }

    #[test]
    fn pragma_6809_takes_the_6809s_alone() {
        let assembly = assemble("\tpragma 6809\n\tldq #1\n\tpragma 6309\n\tldq #1\n");
        assert_eq!(assembly.problems.len(), 1, "{:?}", assembly.problems);
        assert!(assembly.problems[0].message.contains("PRAGMA 6309"));
        // Plain 6809 code is the same either way, and E and W aren't registers to it.
        assert!(!assemble("\tpragma 6809\n\ttfr w,x\n").problems.is_empty());
        // Disassembled as a 6809, the 6309's bytes are data.
        assert_eq!(crate::disasm::m6809(&|i| [0x10u8, 0x30, 0x89][i.min(2)], 0, false).text, "fcb $10");
    }
}
