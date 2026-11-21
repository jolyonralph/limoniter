//! The memory management units' (MMUs') instructions, as vasm writes them in Motorola syntax with nothing
//! optimised: the 68851 PMMU beside a 68020 (an A2620's), the 68030's own MMU (an A3000's or a Falcon's: PMOVE,
//! PFLUSH, PLOAD and PTEST, as the 68851's but fewer), the 68040's (PFLUSH (An), PTEST (An)) and the 68060's (PLPA).
//!
//! Which MMU is chosen as vasm chooses it: MACHINE 68851 (MC68851, PhxAss's PMMU, Devpac's OPT P=68020/68851) or the
//! setting beside the processor gives the 68851; MACHINE 68030, 68040 and 68060 bring their own, and then the
//! 68851's choosing changes nothing. Each instruction, and each form of one, is refused on an MMU that hasn't got
//! it, saying which has.
//!
//! Where vasm takes what the chips wouldn't do, Limoniter refuses instead: the 68851's own registers and
//! instructions on a 68030 or 68040 (which vasm takes if MACHINE 68851 has been seen too, though neither can have
//! a 68851 beside it), and PTEST with level 0 and an address register (Motorola's manuals say level 0 searches only
//! the address translation cache, with the A field and register 0, as there's no descriptor to give the address
//! of).

use super::float;
use super::{Ea, Line, Mmu, Processor, Size, extension, operand, processor, register, settle, split_operands};

/// An MMU, as the instructions it has differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    M68851,
    M68030,
    M68040,
    M68060,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::M68851 => "68851",
            Kind::M68030 => "68030",
            Kind::M68040 => "68040",
            Kind::M68060 => "68060",
        }
    }
}

/// The MMU a processor has with this chosen beside it: its own, or the 68851 (with any but the 68030, 68040 and
/// 68060, as vasm takes it, though only a 68020 has the coprocessor interface for one).
pub fn kind(cpu: Processor, chosen: Mmu) -> Option<Kind> {
    match cpu {
        Processor::M68030 => Some(Kind::M68030),
        Processor::M68040 => Some(Kind::M68040),
        Processor::M68060 => Some(Kind::M68060),
        _ if chosen == Mmu::M68851 => Some(Kind::M68851),
        _ => None,
    }
}

/// The MMU code at this line is for.
pub fn on_line(line: &Line) -> Option<Kind> {
    let chosen = line.setting(super::MMU_SETTING).and_then(Mmu::from_id).unwrap_or_default();
    kind(processor(line), chosen)
}

/// The 68851's 16 conditions, by their codes: B, L, S, A, W, I, G and C, set or clear.
pub const CONDITIONS: [&str; 16] = ["bs", "bc", "ls", "lc", "ss", "sc", "as", "ac", "ws", "wc", "is", "ic", "gs", "gc", "cs", "cc"];

pub fn condition(name: &str) -> Option<u16> {
    CONDITIONS.iter().position(|c| *c == name).map(|c| c as u16)
}

/// The instructions without conditions.
const NAMES: [&str; 17] = ["pmove", "pmovefd", "pflush", "pflusha", "pflushn", "pflushan", "pflushs", "pflushr", "ploadr", "ploadw", "ptestr", "ptestw", "pvalid", "psave", "prestore", "plpar", "plpaw"];

/// Whether a mnemonic (without its size) is an MMU's.
pub fn is_instruction(name: &str) -> bool {
    NAMES.contains(&name)
        || name.strip_prefix("pb").and_then(condition).is_some()
        || name.strip_prefix("pdb").and_then(condition).is_some()
        || name.strip_prefix("ps").and_then(condition).is_some()
        || name.strip_prefix("ptrap").and_then(condition).is_some()
}

/// Every MMU mnemonic, for highlighting and completing.
pub fn mnemonics() -> Vec<String> {
    let mut all: Vec<String> = NAMES.iter().map(|n| n.to_string()).collect();
    for c in CONDITIONS {
        all.extend([format!("pb{c}"), format!("pdb{c}"), format!("ps{c}"), format!("ptrap{c}")]);
    }
    all
}

/// A register PMOVE moves: its bits in the command word (moving into it), its size, and which MMUs have it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Register {
    pub name: &'static str,
    pub bits: u16,
    pub size: char,
    pub has: &'static [Kind],
}

const BOTH: &[Kind] = &[Kind::M68030, Kind::M68851];
const PMMU: &[Kind] = &[Kind::M68851];
const ONLY_030: &[Kind] = &[Kind::M68030];

/// PMOVE's registers. The 68851's MMUSR is its PSR, as on the 68030; its BAD and BAC registers are numbered.
pub const REGISTERS: [Register; 14] = [
    Register { name: "tc", bits: 0x4000, size: 'l', has: BOTH },
    Register { name: "drp", bits: 0x4400, size: 'q', has: PMMU },
    Register { name: "srp", bits: 0x4800, size: 'q', has: BOTH },
    Register { name: "crp", bits: 0x4C00, size: 'q', has: BOTH },
    Register { name: "cal", bits: 0x5000, size: 'b', has: PMMU },
    Register { name: "val", bits: 0x5400, size: 'b', has: PMMU },
    Register { name: "scc", bits: 0x5800, size: 'b', has: PMMU },
    Register { name: "ac", bits: 0x5C00, size: 'w', has: PMMU },
    Register { name: "mmusr", bits: 0x6000, size: 'w', has: BOTH },
    Register { name: "psr", bits: 0x6000, size: 'w', has: BOTH },
    Register { name: "pcsr", bits: 0x6400, size: 'w', has: PMMU },
    Register { name: "tt0", bits: 0x0800, size: 'l', has: ONLY_030 },
    Register { name: "tt1", bits: 0x0C00, size: 'l', has: ONLY_030 },
    // BAD0 to BAD7 and BAC0 to BAC7 are made from this one.
    Register { name: "bad", bits: 0x7000, size: 'w', has: PMMU },
];

/// A PMOVE register by its name, BAD0 to BAD7 and BAC0 to BAC7 among them.
pub fn pmove_register(text: &str) -> Option<Register> {
    let lower = text.trim().to_ascii_lowercase();
    if let Some(found) = REGISTERS.iter().find(|r| r.name == lower && r.name != "bad") {
        return Some(*found);
    }
    let numbered = |prefix: &str, bits: u16| -> Option<Register> {
        let n: u16 = lower.strip_prefix(prefix)?.parse().ok().filter(|n| *n < 8)?;
        (lower.len() == 4).then_some(Register { name: "bad", bits: bits | n << 2, size: 'w', has: PMMU })
    };
    numbered("bad", 0x7000).or_else(|| numbered("bac", 0x7400))
}

/// The name written for a PMOVE register from its bits (moving into it): MMUSR's, not PSR's.
pub fn register_name(bits: u16) -> Option<String> {
    match bits & 0xFC00 {
        0x7000 => Some(format!("bad{}", bits >> 2 & 7)),
        0x7400 => Some(format!("bac{}", bits >> 2 & 7)),
        _ => REGISTERS.iter().find(|r| r.bits == bits & 0xFC00 && r.name != "psr" && r.name != "bad").map(|r| r.name.to_string()),
    }
}

/// Anything an address may be: a register, memory, PC-relative or a number (the 68851's "all").
fn is_any(_: &Ea) -> bool {
    true
}

/// What may be written: a register or memory, not PC-relative or a number.
fn is_alterable(ea: &Ea) -> bool {
    matches!(ea, Ea::D(_) | Ea::A(_)) || ea.is_memory_alterable()
}

fn is_control_alterable(ea: &Ea) -> bool {
    ea.is_control() && !ea.is_pc()
}

/// Memory, PC-relative memory or a number: not a register.
fn is_memory_or_number(ea: &Ea) -> bool {
    ea.is_memory() || matches!(ea, Ea::Immediate(_))
}

/// The mnemonic's name and the size given, if any.
fn split(mnemonic: &str) -> (&str, Option<char>) {
    match mnemonic.rsplit_once('.') {
        Some((name, size)) if size.len() == 1 => (name, size.chars().next()),
        _ => (mnemonic, None),
    }
}

/// Which MMUs have an instruction in the form it's written.
fn who_has(name: &str, texts: &[String]) -> &'static [Kind] {
    use Kind::*;
    let one_an = texts.len() == 1 && matches!(operand(&texts[0]), Ok(Ea::Indirect(_)));
    match name {
        "pflusha" => &[M68030, M68851, M68040, M68060],
        "pflush" if one_an => &[M68040, M68060],
        "pflushn" | "pflushan" => &[M68040, M68060],
        "ptestr" | "ptestw" if one_an => &[M68040],
        "plpar" | "plpaw" => &[M68060],
        "pmovefd" => &[M68030],
        "pmove" => {
            // The registers it moves to or from say who has it.
            let registers: Vec<Register> = texts.iter().filter_map(|t| pmove_register(t)).collect();
            match registers.as_slice() {
                [r] => r.has,
                _ => BOTH,
            }
        }
        "pflush" | "ploadr" | "ploadw" | "ptestr" | "ptestw" => BOTH,
        _ => PMMU,
    }
}

/// The names of MMUs, as a list.
fn names(kinds: &[Kind]) -> String {
    kinds.iter().map(|k| k.name()).collect::<Vec<_>>().join("/")
}

/// A function code operand, as PFLUSH, PLOAD and PTEST take it: SFC, DFC, Dn, or a number (0 to 7 on the 68030, 0
/// to 15 on the 68851).
fn function_code(text: &str, line: &mut Line, kind: Kind) -> Result<u16, String> {
    let lower = text.trim().to_ascii_lowercase();
    match lower.as_str() {
        "sfc" => return Ok(0),
        "dfc" => return Ok(1),
        _ => {}
    }
    if let Some(Ea::D(r)) = register(&lower) {
        return Ok(0x08 | r);
    }
    let Some(value) = text.trim().strip_prefix('#') else {
        return Err(crate::i18n::tf!("A function code is SFC, DFC, a data register or a number (#5), and {text} isn't one"));
    };
    let most = if kind == Kind::M68851 { 15 } else { 7 };
    let v = line.eval(value)?;
    if line.checking() && !(0..=most).contains(&v) {
        return Err(crate::i18n::tf!("A function code is 0 to {most} here, and {v} isn't"));
    }
    Ok(0x10 | (v & most) as u16)
}

/// PFLUSH's mask, after #: 0 to 7 on the 68030, 0 to 15 on the 68851.
fn mask(text: &str, line: &mut Line, kind: Kind) -> Result<u16, String> {
    let Some(value) = text.trim().strip_prefix('#') else {
        return Err(crate::i18n::tf!("PFLUSH's mask is a number (#3), and {text} isn't"));
    };
    let most = if kind == Kind::M68851 { 15 } else { 7 };
    let v = line.eval(value)?;
    if line.checking() && !(0..=most).contains(&v) {
        return Err(crate::i18n::tf!("PFLUSH's mask is 0 to {most} here, and {v} isn't"));
    }
    Ok((v & most) as u16)
}

/// PTEST's level, after #: 0 to 7.
fn level(text: &str, line: &mut Line) -> Result<u16, String> {
    let Some(value) = text.trim().strip_prefix('#') else {
        return Err(crate::i18n::tf!("PTEST's level is a number (#7), and {text} isn't"));
    };
    let v = line.eval(value)?;
    if line.checking() && !(0..=7).contains(&v) {
        return Err(crate::i18n::tf!("PTEST's level is 0 to 7, and {v} isn't"));
    }
    Ok((v & 7) as u16)
}

pub fn encode(mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u16>, String> {
    let (name, size) = split(mnemonic);
    let upper = mnemonic.to_ascii_uppercase();
    let bare = name.to_ascii_uppercase();
    let cpu = processor(line);
    let here = line.here();
    let texts = split_operands(operands);
    let what = if operands.trim().is_empty() { bare.clone() } else { format!("{bare} {}", operands.trim().to_ascii_uppercase()) };
    let has = who_has(name, &texts);
    let Some(kind) = on_line(line).filter(|k| has.contains(k)) else {
        let them = names(has);
        let first = has[0];
        let choose = if first == Kind::M68851 { "MACHINE 68020 and MACHINE 68851".to_string() } else { format!("MACHINE {}", first.name()) };
        return Err(match on_line(line) {
            None if has.contains(&Kind::M68851) => {
                let cpu_name = cpu.name();
                crate::i18n::tf!("{bare} is a memory management unit's (an MMU's) instruction, and this {cpu_name} has none: MACHINE 68851 (or PMMU, or OPT P=68020/68851) gives it the 68851, as does the setting beside \"68000 source is for\"; the 68030, 68040 and 68060 have their own")
            }
            Some(here) => {
                let here = here.name();
                crate::i18n::tf!("{what} is the {them}'s, and this is for the {here}'s MMU: {choose} chooses one")
            }
            None => {
                let here = cpu.name();
                crate::i18n::tf!("{what} is the {them}'s, and this is for the {here}: {choose} chooses one")
            }
        });
    };
    let wrong = || Err(crate::i18n::tf!("{upper} {operands} isn't an instruction of the {}'s MMU", kind.name()));
    let no_size = || -> Result<(), String> {
        match size {
            None => Ok(()),
            Some(_) => Err(crate::i18n::tf!("{bare} has no size")),
        }
    };
    // The 68040's and 68060's: one word, with an address register.
    if matches!(kind, Kind::M68040 | Kind::M68060) {
        no_size()?;
        let an = |texts: &[String]| -> Result<u16, String> {
            match texts.iter().map(|t| operand(t)).collect::<Result<Vec<_>, _>>()?.as_slice() {
                [Ea::Indirect(r)] => Ok(*r),
                _ => Err(crate::i18n::tf!("{bare} takes an address register in brackets, such as (a0)")),
            }
        };
        return match name {
            "pflushn" => Ok(vec![0xF500 | an(&texts)?]),
            "pflush" => Ok(vec![0xF508 | an(&texts)?]),
            "pflushan" | "pflusha" if !texts.is_empty() => Err(crate::i18n::tf!("{bare} takes no operands")),
            "pflushan" => Ok(vec![0xF510]),
            "pflusha" => Ok(vec![0xF518]),
            "ptestw" => Ok(vec![0xF548 | an(&texts)?]),
            "ptestr" => Ok(vec![0xF568 | an(&texts)?]),
            "plpaw" => Ok(vec![0xF588 | an(&texts)?]),
            "plpar" => Ok(vec![0xF5C8 | an(&texts)?]),
            _ => wrong(),
        };
    }
    let pmmu = kind == Kind::M68851;
    // Where vasm reckons a PC-relative displacement from in choosing its size: after the command word, but for
    // PSAVE, PRESTORE and the branches, which haven't one.
    let words = if matches!(name, "psave" | "prestore") || name.starts_with("pb") { 1 } else { 2 };
    let cpc = here + 2 * words;
    // The address in an instruction, settled for the processor.
    let address = |text: &str, line: &mut Line| -> Result<Ea, String> { settle(operand(text)?, text, line, cpc, cpu) };
    // The opcode word and the command word, then an address's extension words.
    let with = |ea: &Ea, command: u16, size: Size, line: &mut Line| -> Result<Vec<u16>, String> {
        let mut words = vec![0xF000 | ea.bits(), command];
        words.extend(extension(ea, size, line, here + 4)?);
        Ok(words)
    };
    let refuse = |text: &str| Err(crate::i18n::tf!("{bare} can't use {text}"));
    match name {
        "pmove" | "pmovefd" => {
            let [from, to] = texts.as_slice() else { return wrong() };
            let (register, other, into) = match (pmove_register(from), pmove_register(to)) {
                (None, Some(r)) => (r, from, true),
                (Some(r), None) => (r, to, false),
                _ => return wrong(),
            };
            if !register.has.contains(&kind) {
                return wrong();
            }
            if let Some(given) = size {
                let fits = given == register.size || (register.size == 'q' && given == 'd');
                if !fits {
                    let wanted = register.size.to_ascii_uppercase();
                    let reg = register_name(register.bits).unwrap_or_default().to_ascii_uppercase();
                    return Err(crate::i18n::tf!("PMOVE of {reg} is .{wanted}"));
                }
            }
            let ea = address(other, line)?;
            let quad = register.size == 'q';
            // The 68030 moves its registers only to and from memory that can be written; the 68851 its root
            // pointers to memory the same way and from memory or a number, and its others to and from anything
            // (but a number or PC-relative, as somewhere to put it).
            let fits = match (pmmu, quad, into) {
                (false, _, _) => is_control_alterable(&ea),
                (true, true, true) => is_memory_or_number(&ea),
                (true, true, false) => is_control_alterable(&ea),
                (true, false, true) => is_any(&ea),
                (true, false, false) => is_alterable(&ea),
            };
            if !fits || (register.name == "pcsr" && into) {
                return refuse(other);
            }
            let fd = if name == "pmovefd" {
                if !into || matches!(register.name, "mmusr" | "psr") {
                    return Err(crate::i18n::tr("PMOVEFD moves into TC, SRP, CRP, TT0 or TT1 only, leaving the address translation cache as it is").into());
                }
                0x100
            } else {
                0
            };
            let command = register.bits | if into { 0 } else { 0x200 } | fd;
            if let Ea::Immediate(value) = &ea {
                let bytes = match register.size {
                    'q' => {
                        let mut eval = |t: &str| line.eval(t);
                        float::whole(value, 8, &mut eval)?
                    }
                    'l' => {
                        let v = line.eval(value)?;
                        (v as u32).to_be_bytes().to_vec()
                    }
                    letter => {
                        let v = line.eval(value)?;
                        let range = if letter == 'b' { -128..=255 } else { -32768..=65535 };
                        if line.checking() && !range.contains(&v) {
                            let letter = letter.to_ascii_uppercase();
                            return Err(crate::i18n::tf!("{v} doesn't fit in .{letter}"));
                        }
                        if letter == 'b' { vec![0, v as u8] } else { (v as u16).to_be_bytes().to_vec() }
                    }
                };
                let mut words = vec![0xF000 | ea.bits(), command];
                words.extend(bytes.chunks(2).map(|w| u16::from_be_bytes([w[0], w[1]])));
                return Ok(words);
            }
            with(&ea, command, Size::Long, line)
        }
        "pflusha" => {
            no_size()?;
            if !texts.is_empty() {
                return Err(crate::i18n::tf!("{bare} takes no operands"));
            }
            Ok(vec![0xF000, 0x2400])
        }
        "pflush" | "pflushs" => {
            no_size()?;
            let (fc, mask, ea) = match texts.as_slice() {
                [fc, mask] => (fc, mask, None),
                [fc, mask, ea] => (fc, mask, Some(ea)),
                _ => return wrong(),
            };
            let fc = function_code(fc, line, kind)?;
            let mask = self::mask(mask, line, kind)?;
            let shared = if name == "pflushs" { 0x0400 } else { 0 };
            match ea {
                None => Ok(vec![0xF000, 0x3000 | shared | mask << 5 | fc]),
                Some(text) => {
                    let ea = address(text, line)?;
                    if !is_control_alterable(&ea) {
                        return refuse(text);
                    }
                    with(&ea, 0x3800 | shared | mask << 5 | fc, Size::Long, line)
                }
            }
        }
        "ploadr" | "ploadw" => {
            no_size()?;
            let [fc, text] = texts.as_slice() else { return wrong() };
            let fc = function_code(fc, line, kind)?;
            let ea = address(text, line)?;
            if !is_control_alterable(&ea) {
                return refuse(text);
            }
            with(&ea, 0x2000 | if name == "ploadr" { 0x200 } else { 0 } | fc, Size::Long, line)
        }
        "ptestr" | "ptestw" => {
            no_size()?;
            let (fc, text, level, an) = match texts.as_slice() {
                [fc, ea, level] => (fc, ea, level, None),
                [fc, ea, level, an] => (fc, ea, level, Some(an)),
                _ => return wrong(),
            };
            let fc = function_code(fc, line, kind)?;
            let ea = address(text, line)?;
            if !is_control_alterable(&ea) {
                return refuse(text);
            }
            let level = self::level(level, line)?;
            let an = match an {
                None => 0,
                Some(a) => match register(a) {
                    Some(Ea::A(_)) if level == 0 => {
                        return Err(crate::i18n::tf!("{bare} at level 0 searches only the address translation cache, so it can't give a descriptor's address in {a}"));
                    }
                    Some(Ea::A(r)) => 0x100 | r << 5,
                    _ => return Err(crate::i18n::tf!("{bare} gives the descriptor's address in an address register, and {a} isn't one")),
                },
            };
            with(&ea, 0x8000 | level << 10 | if name == "ptestr" { 0x200 } else { 0 } | an | fc, Size::Long, line)
        }
        "pvalid" => {
            no_size()?;
            let [first, text] = texts.as_slice() else { return wrong() };
            let reg = if first.trim().eq_ignore_ascii_case("val") {
                0x2800
            } else {
                match register(first) {
                    Some(Ea::A(r)) => 0x2C00 | r,
                    _ => return Err(crate::i18n::tf!("PVALID compares with VAL or an address register, and {first} isn't one")),
                }
            };
            let ea = address(text, line)?;
            if !is_control_alterable(&ea) {
                return refuse(text);
            }
            with(&ea, reg, Size::Long, line)
        }
        "pflushr" => {
            if size.is_some_and(|s| s != 'q' && s != 'd') {
                return Err(crate::i18n::tr("PFLUSHR's root pointer is .Q").into());
            }
            let [text] = texts.as_slice() else { return wrong() };
            let ea = address(text, line)?;
            if !is_memory_or_number(&ea) {
                return refuse(text);
            }
            if let Ea::Immediate(value) = &ea {
                let mut eval = |t: &str| line.eval(t);
                let bytes = float::whole(value, 8, &mut eval)?;
                let mut words = vec![0xF000 | ea.bits(), 0xA000];
                words.extend(bytes.chunks(2).map(|w| u16::from_be_bytes([w[0], w[1]])));
                return Ok(words);
            }
            with(&ea, 0xA000, Size::Long, line)
        }
        "psave" | "prestore" => {
            no_size()?;
            let [text] = texts.as_slice() else { return wrong() };
            let ea = address(text, line)?;
            let fits = if name == "psave" { matches!(ea, Ea::PreDecrement(_)) || is_control_alterable(&ea) } else { matches!(ea, Ea::PostIncrement(_)) || ea.is_control() };
            if !fits {
                return refuse(text);
            }
            let mut words = vec![if name == "psave" { 0xF100 } else { 0xF140 } | ea.bits()];
            words.extend(extension(&ea, Size::Long, line, here + 2)?);
            Ok(words)
        }
        _ => conditional(name, size, &texts, operands, line, &bare),
    }
}

/// The 68851's conditional instructions: PBcc, PDBcc, PScc and PTRAPcc.
fn conditional(name: &str, size: Option<char>, texts: &[String], operands: &str, line: &mut Line, bare: &str) -> Result<Vec<u16>, String> {
    let here = line.here();
    let cpu = processor(line);
    let wrong = || Err(crate::i18n::tf!("{bare} {operands} isn't a 68851 instruction"));
    if let Some(cc) = name.strip_prefix("pb").and_then(condition) {
        let target = line.eval(operands)?;
        let offset = target - (here + 2);
        return match size {
            Some('l') => Ok(vec![0xF0C0 | cc, (offset >> 16) as u16, offset as u16]),
            None | Some('w') => {
                if line.checking() && !(-32768..=32767).contains(&offset) {
                    return Err(crate::i18n::tf!("{operands} is too far away for {bare}.W: {bare}.L reaches it"));
                }
                Ok(vec![0xF080 | cc, offset as u16])
            }
            Some(_) => Err(crate::i18n::tf!("{bare} is .W or .L")),
        };
    }
    if let Some(cc) = name.strip_prefix("pdb").and_then(condition) {
        if size.is_some_and(|s| s != 'w') {
            return Err(crate::i18n::tf!("{bare} is .W"));
        }
        let [counter, target] = texts else { return wrong() };
        let Some(Ea::D(r)) = register(counter) else { return wrong() };
        let offset = line.eval(target)? - (here + 4);
        if line.checking() && !(-32768..=32767).contains(&offset) {
            return Err(crate::i18n::tf!("{target} is too far away for {bare}"));
        }
        return Ok(vec![0xF048 | r, cc, offset as u16]);
    }
    if let Some(cc) = name.strip_prefix("ptrap").and_then(condition) {
        return match (texts, size) {
            ([], None | Some('w' | 'l')) => Ok(vec![0xF07C, cc]),
            ([v], None | Some('w')) if v.trim().starts_with('#') => Ok(vec![0xF07A, cc, line.eval(&v.trim()[1..])? as u16]),
            ([v], Some('l')) if v.trim().starts_with('#') => {
                let v = line.eval(&v.trim()[1..])?;
                Ok(vec![0xF07B, cc, (v >> 16) as u16, v as u16])
            }
            _ => wrong(),
        };
    }
    if let Some(cc) = name.strip_prefix("ps").and_then(condition) {
        if size.is_some_and(|s| s != 'b') {
            return Err(crate::i18n::tf!("{bare} is .B"));
        }
        let [text] = texts else { return wrong() };
        let ea = settle(operand(text)?, text, line, here + 4, cpu)?;
        if !(matches!(ea, Ea::D(_)) || ea.is_memory_alterable()) {
            return Err(crate::i18n::tf!("{bare} can't write to {text}"));
        }
        let mut words = vec![0xF040 | ea.bits(), cc];
        words.extend(extension(&ea, Size::Byte, line, here + 4)?);
        return Ok(words);
    }
    wrong()
}

#[cfg(test)]
mod tests;
