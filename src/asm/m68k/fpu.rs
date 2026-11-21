//! The 68881's and 68882's instructions, as vasm writes them in Motorola syntax with nothing optimised: the
//! maths coprocessor beside a 68020 or 68030 (an A2630's, an A3000's), and the FPU built into the 68040 and 68060,
//! which take the same instructions here.
//!
//! The FPU is chosen as vasm chooses it: MACHINE 68881 or 68882 (MC68881, FPU 1, Devpac's OPT P=68020/68881), or
//! the setting beside the processor in Limoniter's settings; MACHINE 68040 and 68060 bring their own. Without one,
//! each FPU instruction is refused, saying how to choose one.
//!
//! With no size, an address or a data register is a word, as vasm takes it (and as the 680x0's own instructions
//! take it, though some assemblers take .X for the FPU's): FMOVE (A0),FP0 is FMOVE.W. Between FP registers it's
//! always .X, the control registers .L, and FMOVEM's FP registers .X.
//!
//! Where vasm does something the FPU wouldn't make sense of, Limoniter refuses instead: FMOVE.L #2.7,FP0 (vasm
//! writes 2.7's single-precision bits as the whole number), and FNOP with an operand (which vasm drops).

use super::float::{self, Format};
use super::{Ea, Line, Processor, Size, extension, operand, processor, register, settle, split_operands};

/// The instructions that take one operand and work on it (or take it from memory into a register): their
/// opmodes. FMOVE is one of them, from memory or a register into an FP register.
pub const MONADIC: [(&str, u16); 26] = [
    ("fmove", 0x00),
    ("fint", 0x01),
    ("fsinh", 0x02),
    ("fintrz", 0x03),
    ("fsqrt", 0x04),
    ("flognp1", 0x06),
    ("fetoxm1", 0x08),
    ("ftanh", 0x09),
    ("fatan", 0x0A),
    ("fasin", 0x0C),
    ("fatanh", 0x0D),
    ("fsin", 0x0E),
    ("ftan", 0x0F),
    ("fetox", 0x10),
    ("ftwotox", 0x11),
    ("ftentox", 0x12),
    ("flogn", 0x14),
    ("flog10", 0x15),
    ("flog2", 0x16),
    ("fabs", 0x18),
    ("fcosh", 0x19),
    ("fneg", 0x1A),
    ("facos", 0x1C),
    ("fcos", 0x1D),
    ("fgetexp", 0x1E),
    ("fgetman", 0x1F),
];

/// The instructions that take a source and a destination register: their opmodes.
pub const DYADIC: [(&str, u16); 10] = [("fdiv", 0x20), ("fmod", 0x21), ("fadd", 0x22), ("fmul", 0x23), ("fsgldiv", 0x24), ("frem", 0x25), ("fscale", 0x26), ("fsglmul", 0x27), ("fsub", 0x28), ("fcmp", 0x38)];

/// The 68040's and 68060's own: FMOVE, FSQRT, FABS, FNEG and the four sums with their results rounded to single
/// (FS) or double (FD) precision, whatever FPCR says. The 68881 and 68882 haven't them.
pub const ROUNDED: [(&str, u16); 16] = [
    ("fsmove", 0x40),
    ("fssqrt", 0x41),
    ("fdmove", 0x44),
    ("fdsqrt", 0x45),
    ("fsabs", 0x58),
    ("fsneg", 0x5A),
    ("fdabs", 0x5C),
    ("fdneg", 0x5E),
    ("fsdiv", 0x60),
    ("fsadd", 0x62),
    ("fsmul", 0x63),
    ("fddiv", 0x64),
    ("fdadd", 0x66),
    ("fdmul", 0x67),
    ("fssub", 0x68),
    ("fdsub", 0x6C),
];

/// Whether an instruction with this opmode works on a single FP register when it's given one (FABS FP0): the
/// monadic ones (the rounding ones below $60), but for the moves.
pub fn takes_one(opmode: u16) -> bool {
    MONADIC.iter().chain(ROUNDED.iter().filter(|(_, op)| *op < 0x60)).any(|(name, op)| *op == opmode && !name.ends_with("move"))
}

pub const FSINCOS: u16 = 0x30;
pub const FTST: u16 = 0x3A;

/// The FPU's 32 conditions, by their codes.
pub const CONDITIONS: [&str; 32] = [
    "f", "eq", "ogt", "oge", "olt", "ole", "ogl", "or", "un", "ueq", "ugt", "uge", "ult", "ule", "ne", "t", "sf", "seq", "gt", "ge", "lt", "le", "gl", "gle", "ngle", "ngl", "nle", "nlt", "nge", "ngt", "sne", "st",
];

/// The control registers, as FMOVE's and FMOVEM's register select bits name them.
pub const CONTROL: [(&str, u16); 3] = [("fpcr", 4), ("fpsr", 2), ("fpiar", 1)];

pub fn condition(name: &str) -> Option<u16> {
    CONDITIONS.iter().position(|c| *c == name).map(|c| c as u16)
}

fn opmode(name: &str) -> Option<u16> {
    MONADIC.iter().chain(&DYADIC).chain(&ROUNDED).find(|(n, _)| *n == name).map(|(_, op)| *op)
}

/// Whether a mnemonic (without its size) is one of the FPU's.
pub fn is_instruction(name: &str) -> bool {
    matches!(name, "fmovem" | "fmovecr" | "fsincos" | "ftst" | "fnop" | "fsave" | "frestore" | "fdbra")
        || opmode(name).is_some()
        || name.strip_prefix("fb").and_then(condition).is_some()
        || name.strip_prefix("fdb").and_then(condition).is_some()
        || name.strip_prefix("fs").and_then(condition).is_some()
        || name.strip_prefix("ftrap").and_then(condition).is_some()
}

/// Every FPU mnemonic, for highlighting and completing.
pub fn mnemonics() -> Vec<String> {
    let mut all: Vec<String> = ["fmovem", "fmovecr", "fsincos", "ftst", "fnop", "fsave", "frestore", "fdbra"].iter().map(|s| s.to_string()).collect();
    all.extend(MONADIC.iter().chain(&DYADIC).chain(&ROUNDED).map(|(n, _)| n.to_string()));
    for c in CONDITIONS {
        all.extend([format!("fb{c}"), format!("fdb{c}"), format!("fs{c}"), format!("ftrap{c}")]);
    }
    all
}

/// The FPU code at this line is for, and its coprocessor number: None if there's none.
pub fn coprocessor(line: &Line) -> Option<u16> {
    let cpu = processor(line);
    let built_in = matches!(cpu, Processor::M68040 | Processor::M68060);
    match line.setting(super::FPU_SETTING).unwrap_or(0) {
        // FPU 0 turned it off.
        -1 => None,
        0 if !built_in => None,
        _ if built_in => Some(1),
        _ => Some(line.setting(super::FPU_ID_SETTING).unwrap_or(1).clamp(1, 7) as u16),
    }
}

/// An operand of an FPU instruction.
#[derive(Clone, Debug, PartialEq)]
enum Op {
    Fp(u16),
    /// Control registers: FPCR 4, FPSR 2, FPIAR 1.
    Control(u16),
    /// FP registers: bit n is FPn.
    List(u16),
    /// FSINCOS's FPc:FPs.
    Pair(u16, u16),
    Ea(Ea),
}

fn fp_register(text: &str) -> Option<u16> {
    let lower = text.trim().to_ascii_lowercase();
    let n: u16 = lower.strip_prefix("fp")?.parse().ok()?;
    (n < 8 && lower.len() == 3).then_some(n)
}

fn control_register(text: &str) -> Option<u16> {
    let lower = text.trim().to_ascii_lowercase();
    CONTROL.iter().find(|(n, _)| *n == lower).map(|(_, bit)| *bit)
}

/// FP0-FP3/FP5, as vasm also takes FP1-3.
fn fp_list(text: &str) -> Option<u16> {
    let mut mask = 0;
    for part in text.split('/') {
        match part.split_once('-') {
            Some((from, to)) => {
                let from = fp_register(from)?;
                let to = fp_register(to).or_else(|| to.trim().parse::<u16>().ok().filter(|n| *n < 8))?;
                for n in from.min(to)..=from.max(to) {
                    mask |= 1 << n;
                }
            }
            None => mask |= 1 << fp_register(part)?,
        }
    }
    Some(mask)
}

fn control_list(text: &str) -> Option<u16> {
    text.split('/').map(control_register).try_fold(0, |mask, bit| bit.map(|b| mask | b))
}

fn fop(text: &str) -> Result<Op, String> {
    if let Some(n) = fp_register(text) {
        return Ok(Op::Fp(n));
    }
    if let Some(bits) = control_list(text) {
        return Ok(Op::Control(bits));
    }
    if let Some((c, s)) = text.split_once(':')
        && let (Some(c), Some(s)) = (fp_register(c), fp_register(s))
    {
        return Ok(Op::Pair(c, s));
    }
    if (text.contains('/') || text.contains('-')) && text.trim_start().to_ascii_lowercase().starts_with("fp")
        && let Some(mask) = fp_list(text)
    {
        return Ok(Op::List(mask));
    }
    Ok(Op::Ea(operand(text)?))
}

/// A mnemonic's name and its size, if it has one.
fn split_format(mnemonic: &str) -> Result<(&str, Option<Format>), String> {
    match mnemonic.rsplit_once('.') {
        Some((name, letter)) => match Format::from_letter(letter) {
            Some(format) => Ok((name, Some(format))),
            None => {
                let upper = mnemonic.to_ascii_uppercase();
                Err(crate::i18n::tf!("{upper} has no such size: the FPU's are .B, .W, .L, .S, .D, .X and .P"))
            }
        },
        None => Ok((mnemonic, None)),
    }
}

/// An immediate's words in a format: a whole number for .B, .W and .L, a floating-point one for the others.
fn immediate(text: &str, format: Format, line: &mut Line) -> Result<Vec<u16>, String> {
    if !format.is_float() && float::starts_with_decimal(text) && text.contains(['.', 'e', 'E']) && !text.trim_start_matches(['-', '+', '(', ' ']).starts_with("0x") {
        let letter = format.letter().to_ascii_uppercase();
        return Err(crate::i18n::tf!(".{letter} takes a whole number, and {text} isn't one: .S, .D, .X and .P take floating-point numbers"));
    }
    let mut eval = |t: &str| line.eval(t);
    let mut bytes = float::constant(text, format, &mut eval)?;
    if bytes.len() == 1 {
        bytes.insert(0, 0);
    }
    Ok(bytes.chunks(2).map(|w| u16::from_be_bytes([w[0], w[1]])).collect())
}

/// An address's extension words, at `at`: an immediate in the instruction's format, anything else as ever.
fn ea_words(ea: &Ea, format: Format, line: &mut Line, at: i64) -> Result<Vec<u16>, String> {
    match ea {
        Ea::Immediate(text) => immediate(text, format, line),
        _ => extension(ea, Size::Long, line, at),
    }
}

fn is_data_alterable(ea: &Ea) -> bool {
    matches!(ea, Ea::D(_)) || ea.is_memory_alterable()
}

fn is_control_alterable(ea: &Ea) -> bool {
    ea.is_control() && !ea.is_pc()
}

pub fn encode(mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u16>, String> {
    let (name, format) = split_format(mnemonic)?;
    let upper = mnemonic.to_ascii_uppercase();
    let bare = name.to_ascii_uppercase();
    let cpu = processor(line);
    if ROUNDED.iter().any(|(n, _)| *n == name) && !matches!(cpu, Processor::M68040 | Processor::M68060) {
        let cpu_name = cpu.name();
        return Err(crate::i18n::tf!("{bare} is the 68040's and 68060's own FPU's (the 68881 and 68882 haven't got it), and this is for the {cpu_name}: MACHINE 68040 (or 68060) chooses one"));
    }
    let Some(id) = coprocessor(line) else {
        let cpu_name = cpu.name();
        return Err(crate::i18n::tf!("{bare} is a maths coprocessor's (an FPU's) instruction, and none is chosen for this {cpu_name}: MACHINE 68881 or MACHINE 68882 (or FPU 1, or OPT P=68020/68881) chooses one, as does the setting beside \"68000 source is for\"; the 68040 and 68060 have their own"));
    };
    let here = line.here();
    let wrong = || Err(crate::i18n::tf!("{upper} {operands} isn't an FPU instruction"));
    let base = 0xF000 | id << 9;
    let mut texts = split_operands(operands);
    // FMOVE.P's k-factor, after its destination: {#k} or {Dn}.
    let mut k_factor: Option<String> = None;
    if let Some(last) = texts.last_mut()
        && last.ends_with('}')
        && let Some(open) = last.rfind('{')
    {
        k_factor = Some(last[open + 1..last.len() - 1].trim().to_string());
        *last = last[..open].trim().to_string();
    }
    if k_factor.is_some() && !(name == "fmove" && format == Some(Format::Packed)) {
        return Err(crate::i18n::tf!("Only FMOVE.P takes a k-factor ({{#k}} or {{Dn}}) after its destination"));
    }
    let ops: Vec<Op> = texts.iter().map(|t| fop(t)).collect::<Result<_, _>>()?;
    // Where vasm reckons a PC-relative displacement from in choosing its size: after the command word, but for
    // FSAVE and FRESTORE, which haven't one.
    let words = if matches!(name, "fsave" | "frestore") || name.starts_with("fb") { 1 } else { 2 };
    let cpc = here + 2 * words;
    let mut ops = ops;
    for (op, text) in ops.iter_mut().zip(&texts) {
        if let Op::Ea(ea) = op {
            *ea = settle(ea.clone(), text, line, cpc, cpu)?;
        }
    }
    // The opcode word and command word, then an address's extension words.
    let with = |op: u16, command: u16, ea: Option<(&Ea, Format)>, line: &mut Line| -> Result<Vec<u16>, String> {
        let mut words = vec![op, command];
        if let Some((ea, format)) = ea {
            words.extend(ea_words(ea, format, line, here + 4)?);
        }
        Ok(words)
    };
    let x_only = |what: &str| -> Result<(), String> {
        match format {
            None | Some(Format::Extended) => Ok(()),
            Some(_) => Err(crate::i18n::tf!("{what} is always .X")),
        }
    };
    let long_only = || -> Result<(), String> {
        match format {
            None | Some(Format::Long) => Ok(()),
            Some(_) => Err(crate::i18n::tf!("The FPU's control registers are moved as .L, and {upper} isn't")),
        }
    };
    // Where an FP register's number goes as a source, and as a destination.
    let src = |n: u16| n << 10;
    let dst = |n: u16| n << 7;
    // An address as the source of a calculation: data, but a data register only for what fits in one.
    let source = |ea: &Ea, format: Format| -> Result<(), String> {
        let fits = match ea {
            Ea::D(_) => matches!(format, Format::Byte | Format::Word | Format::Long | Format::Single),
            Ea::A(_) => false,
            _ => ea.is_data(),
        };
        if fits {
            Ok(())
        } else {
            let letter = format.letter().to_ascii_uppercase();
            Err(crate::i18n::tf!("{upper} can't take .{letter} from {}", texts[0]))
        }
    };
    if let Some(op) = opmode(name).or_else(|| (name == "ftst").then_some(FTST)).or_else(|| (name == "fsincos").then_some(FSINCOS)) {
        let one = takes_one(op);
        match (name, ops.as_slice()) {
            // FMOVE out, to memory or a data register; FMOVE.P with its k-factor.
            ("fmove", [Op::Fp(n), Op::Ea(to)]) => {
                let format = format.unwrap_or(Format::Word);
                let fits = match to {
                    Ea::D(_) => matches!(format, Format::Byte | Format::Word | Format::Long | Format::Single),
                    _ => is_data_alterable(to),
                };
                if !fits {
                    let letter = format.letter().to_ascii_uppercase();
                    return Err(crate::i18n::tf!("FMOVE can't write .{letter} to {}", texts[1]));
                }
                let k = match (format, k_factor.as_deref()) {
                    (Format::Packed, Some(k)) => match register(k) {
                        Some(Ea::D(r)) => 0x1C00 | r << 4,
                        _ => {
                            let v = line.eval(k.trim_start_matches('#'))?;
                            if line.checking() && !(-64..=63).contains(&v) {
                                return Err(crate::i18n::tf!("A k-factor is -64 to 63, and {v} isn't"));
                            }
                            v as u16 & 0x7F
                        }
                    },
                    _ => 0,
                };
                // The register is the source here, written where a destination register would be. A dynamic
                // k-factor's format is 7 where a static one's is 3, so the bits of both go together.
                with(base | to.bits(), 0x6000 | format.code() << 10 | dst(*n) | k, Some((to, format)), line)
            }
            // FMOVE to and from the control registers, one at a time.
            ("fmove", [Op::Ea(from), Op::Control(c)]) if c.count_ones() == 1 => {
                long_only()?;
                control_in(base, *c, from, &texts[0], &with, line, &upper)
            }
            ("fmove", [Op::Control(c), Op::Ea(to)]) if c.count_ones() == 1 => {
                long_only()?;
                control_out(base, *c, to, &texts[1], &with, line, &upper)
            }
            (_, [Op::Fp(m), Op::Fp(n)]) if name != "ftst" && name != "fsincos" => {
                x_only(&crate::i18n::tf!("{bare} between FP registers"))?;
                Ok(vec![base, src(*m) | dst(*n) | op])
            }
            ("ftst", [Op::Fp(m)]) => {
                x_only(crate::i18n::tr("FTST of an FP register"))?;
                Ok(vec![base, src(*m) | op])
            }
            (_, [Op::Fp(n)]) if one => {
                x_only(&crate::i18n::tf!("{bare} of an FP register"))?;
                Ok(vec![base, src(*n) | dst(*n) | op])
            }
            ("ftst", [Op::Ea(from)]) => {
                let format = format.unwrap_or(Format::Word);
                source(from, format)?;
                with(base | from.bits(), 0x4000 | format.code() << 10 | op, Some((from, format)), line)
            }
            ("fsincos", [Op::Fp(m), Op::Pair(c, s)]) => {
                x_only(crate::i18n::tr("FSINCOS from an FP register"))?;
                Ok(vec![base, src(*m) | dst(*s) | op | c])
            }
            ("fsincos", [Op::Ea(from), Op::Pair(c, s)]) => {
                let format = format.unwrap_or(Format::Word);
                source(from, format)?;
                with(base | from.bits(), 0x4000 | format.code() << 10 | dst(*s) | op | c, Some((from, format)), line)
            }
            (_, [Op::Ea(from), Op::Fp(n)]) if name != "ftst" && name != "fsincos" => {
                let format = format.unwrap_or(Format::Word);
                source(from, format)?;
                with(base | from.bits(), 0x4000 | format.code() << 10 | dst(*n) | op, Some((from, format)), line)
            }
            _ => wrong(),
        }
    } else if name == "fmovecr" {
        x_only("FMOVECR")?;
        let [Op::Ea(Ea::Immediate(offset)), Op::Fp(n)] = ops.as_slice() else { return wrong() };
        let v = line.eval(offset)?;
        if line.checking() && !(0..=127).contains(&v) {
            return Err(crate::i18n::tf!("FMOVECR's constants are numbered 0 to 127 ($7F), and {v} isn't one"));
        }
        Ok(vec![base, 0x5C00 | n << 7 | (v as u16 & 0x7F)])
    } else if name == "fmovem" {
        match ops.as_slice() {
            [Op::Ea(from), Op::Control(c)] => {
                long_only()?;
                control_in(base, *c, from, &texts[0], &with, line, &upper)
            }
            [Op::Control(c), Op::Ea(to)] => {
                long_only()?;
                control_out(base, *c, to, &texts[1], &with, line, &upper)
            }
            // FP registers to memory: to -(An) the list runs FP7 to FP0 from bit 7, otherwise FP0 to FP7; or a
            // data register holds the list.
            [list @ (Op::Fp(_) | Op::List(_) | Op::Ea(Ea::D(_))), Op::Ea(to)] => {
                x_only("FMOVEM of FP registers")?;
                let pre = matches!(to, Ea::PreDecrement(_));
                if !pre && !is_control_alterable(to) {
                    return Err(crate::i18n::tf!("FMOVEM can only put FP registers at -(An) or an address that can be written, and {} isn't", texts[1]));
                }
                let command = match list {
                    Op::Ea(Ea::D(r)) => 0xE800 | if pre { 0 } else { 0x1000 } | r << 4,
                    Op::Fp(n) => 0xE000 | if pre { 1 << n } else { 0x1000 | 0x80 >> n },
                    Op::List(mask) => 0xE000 | if pre { *mask } else { 0x1000 | (*mask as u8).reverse_bits() as u16 },
                    _ => return wrong(),
                };
                with(base | to.bits(), command, Some((to, Format::Extended)), line)
            }
            [Op::Ea(from), list @ (Op::Fp(_) | Op::List(_) | Op::Ea(Ea::D(_)))] => {
                x_only("FMOVEM of FP registers")?;
                if !matches!(from, Ea::PostIncrement(_)) && !from.is_control() {
                    return Err(crate::i18n::tf!("FMOVEM can only take FP registers from (An)+ or an address, and {} isn't", texts[0]));
                }
                let command = match list {
                    Op::Ea(Ea::D(r)) => 0xD800 | r << 4,
                    Op::Fp(n) => 0xD000 | 0x80 >> n,
                    Op::List(mask) => 0xD000 | (*mask as u8).reverse_bits() as u16,
                    _ => return wrong(),
                };
                with(base | from.bits(), command, Some((from, Format::Extended)), line)
            }
            _ => wrong(),
        }
    } else if name == "fnop" {
        if !ops.is_empty() {
            return Err(crate::i18n::tr("FNOP takes no operands").into());
        }
        Ok(vec![base | 0x80, 0])
    } else if name == "fsave" || name == "frestore" {
        let [Op::Ea(ea)] = ops.as_slice() else { return wrong() };
        let fits = if name == "fsave" { matches!(ea, Ea::PreDecrement(_)) || is_control_alterable(ea) } else { matches!(ea, Ea::PostIncrement(_)) || ea.is_control() };
        if !fits {
            return Err(crate::i18n::tf!("{bare} can't use {}", texts[0]));
        }
        let mut words = vec![base | if name == "fsave" { 0x100 } else { 0x140 } | ea.bits()];
        words.extend(extension(ea, Size::Long, line, here + 2)?);
        Ok(words)
    } else if let Some(cc) = name.strip_prefix("fb").and_then(condition) {
        let target = line.eval(operands)?;
        let offset = target - (here + 2);
        match format {
            Some(Format::Long) => Ok(vec![base | 0xC0 | cc, (offset >> 16) as u16, offset as u16]),
            None | Some(Format::Word) => {
                if line.checking() && !(-32768..=32767).contains(&offset) {
                    return Err(crate::i18n::tf!("{operands} is too far away for {bare}.W: {bare}.L reaches it"));
                }
                Ok(vec![base | 0x80 | cc, offset as u16])
            }
            Some(_) => Err(crate::i18n::tf!("{bare} is .W or .L")),
        }
    } else if let Some(cc) = name.strip_prefix("fdb").and_then(condition).or_else(|| (name == "fdbra").then_some(0)) {
        let [Op::Ea(Ea::D(r)), _] = ops.as_slice() else { return wrong() };
        if format.is_some_and(|f| f != Format::Word) {
            return Err(crate::i18n::tf!("{bare} is .W"));
        }
        let offset = line.eval(&texts[1])? - (here + 4);
        if line.checking() && !(-32768..=32767).contains(&offset) {
            return Err(crate::i18n::tf!("{} is too far away for {bare}", texts[1]));
        }
        Ok(vec![base | 0x48 | r, cc, offset as u16])
    } else if let Some(cc) = name.strip_prefix("ftrap").and_then(condition) {
        match (ops.as_slice(), format) {
            ([], None | Some(Format::Word | Format::Long)) => Ok(vec![base | 0x7C, cc]),
            ([Op::Ea(Ea::Immediate(v))], None | Some(Format::Word)) => Ok(vec![base | 0x7A, cc, line.eval(v)? as u16]),
            ([Op::Ea(Ea::Immediate(v))], Some(Format::Long)) => {
                let v = line.eval(v)?;
                Ok(vec![base | 0x7B, cc, (v >> 16) as u16, v as u16])
            }
            _ => wrong(),
        }
    } else if let Some(cc) = name.strip_prefix("fs").and_then(condition) {
        let [Op::Ea(ea)] = ops.as_slice() else { return wrong() };
        if format.is_some_and(|f| f != Format::Byte) {
            return Err(crate::i18n::tf!("{bare} is .B"));
        }
        if !is_data_alterable(ea) {
            return Err(crate::i18n::tf!("{bare} can't write to {}", texts[0]));
        }
        with(base | 0x40 | ea.bits(), cc, Some((ea, Format::Byte)), line)
    } else {
        wrong()
    }
}

type With<'a> = dyn Fn(u16, u16, Option<(&Ea, Format)>, &mut Line) -> Result<Vec<u16>, String> + 'a;

/// FMOVE or FMOVEM into control registers: one may come from a data register (or FPIAR from an address
/// register); several only from memory or an immediate, a long for each.
fn control_in(base: u16, c: u16, from: &Ea, text: &str, with: &With, line: &mut Line, upper: &str) -> Result<Vec<u16>, String> {
    let one = c.count_ones() == 1;
    let fits = match from {
        Ea::D(_) => one,
        Ea::A(_) => c == 1,
        _ => from.is_data(),
    };
    if !fits {
        return Err(crate::i18n::tf!("{upper} can't move {text} into {}", names(c)));
    }
    let command = 0x8000 | c << 10;
    if let Ea::Immediate(value) = from {
        let mut eval = |t: &str| line.eval(t);
        let bytes = float::whole(value, 4 * c.count_ones() as usize, &mut eval)?;
        let mut words = vec![base | from.bits(), command];
        words.extend(bytes.chunks(2).map(|w| u16::from_be_bytes([w[0], w[1]])));
        return Ok(words);
    }
    with(base | from.bits(), command, Some((from, Format::Long)), line)
}

/// FMOVE or FMOVEM out of control registers: the same, to somewhere that can be written.
fn control_out(base: u16, c: u16, to: &Ea, text: &str, with: &With, line: &mut Line, upper: &str) -> Result<Vec<u16>, String> {
    let one = c.count_ones() == 1;
    let fits = match to {
        Ea::D(_) => one,
        Ea::A(_) => c == 1,
        _ => to.is_memory_alterable(),
    };
    if !fits {
        return Err(crate::i18n::tf!("{upper} can't move {} to {text}", names(c)));
    }
    with(base | to.bits(), 0xA000 | c << 10, Some((to, Format::Long)), line)
}

/// Control registers' names, as a list.
fn names(c: u16) -> String {
    CONTROL.iter().filter(|(_, bit)| c & bit != 0).map(|(n, _)| n.to_ascii_uppercase()).collect::<Vec<_>>().join("/")
}

#[cfg(test)]
mod tests;
