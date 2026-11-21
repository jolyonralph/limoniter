//! The 68040's, 68060's and CPU32's own instructions, as vasm writes them: MOVE16 (a cache line of 16 bytes at
//! once), CINV and CPUSH (the caches' lines, pages or all of them, invalidated or pushed to memory), the 68060's and
//! CPU32's LPSTOP, and the CPU32's table lookups and BGND.
//!
//! The instructions the 68060 dropped (MOVEP, CAS2, CHK2 and CMP2, and the 64-bit MULU.L, MULS.L, DIVU.L and
//! DIVS.L) are assembled for it as vasm assembles them, with no warning: Motorola's 68060 software package does
//! them when the processor traps them, so code for a 68040 still runs.
//!
//! vasm takes CPUSHA DC/IC as CPUSHA DC, warning that it ignores the rest; Limoniter refuses it, as both caches are
//! BC.

use super::{Ea, Line, Need, Size, extension, needs, operand, processor, settle, split_operands};

const NAMES: [&str; 13] = ["move16", "cinvl", "cinvp", "cinva", "cpushl", "cpushp", "cpusha", "lpstop", "tblu", "tbls", "tblun", "tblsn", "bgnd"];

/// The caches CINV and CPUSH name: none, the data cache, the instruction cache, and both.
pub const CACHES: [&str; 4] = ["nc", "dc", "ic", "bc"];

/// The table lookups, by the bits of their kinds.
pub const TABLES: [(&str, u16); 4] = [("tblu", 0x0000), ("tblun", 0x0400), ("tbls", 0x0800), ("tblsn", 0x0C00)];

pub fn is_instruction(name: &str) -> bool {
    NAMES.contains(&name)
}

pub fn mnemonics() -> Vec<String> {
    NAMES.iter().map(|n| n.to_string()).collect()
}

pub fn encode(mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u16>, String> {
    let (name, size) = match mnemonic.rsplit_once('.') {
        Some((name, size)) => (name, Some(size)),
        None => (mnemonic, None),
    };
    let upper = mnemonic.to_ascii_uppercase();
    let bare = name.to_ascii_uppercase();
    let cpu = processor(line);
    let here = line.here();
    let texts = split_operands(operands);
    let wrong = || Err(crate::i18n::tf!("{upper} {operands} isn't a {} instruction", cpu.name()));
    let no_size = || -> Result<(), String> {
        match size {
            None => Ok(()),
            Some(_) => Err(crate::i18n::tf!("{bare} has no size")),
        }
    };
    let cache = |text: &str| -> Result<u16, String> {
        let lower = text.trim().to_ascii_lowercase();
        CACHES.iter().position(|c| *c == lower).map(|c| c as u16).ok_or_else(|| crate::i18n::tf!("{bare} names a cache, DC (data), IC (instruction), BC (both) or NC (none), and {text} isn't one"))
    };
    match name {
        "move16" => {
            needs(cpu, Need::M68040, "MOVE16")?;
            no_size()?;
            let [from, to] = texts.as_slice() else { return wrong() };
            let (from, to) = (operand(from)?, operand(to)?);
            let long = |text: &str, line: &mut Line| -> Result<[u16; 2], String> {
                let v = line.eval(text)?;
                Ok([(v >> 16) as u16, v as u16])
            };
            match (&from, &to) {
                (Ea::PostIncrement(x), Ea::PostIncrement(y)) => Ok(vec![0xF620 | x, 0x8000 | y << 12]),
                (Ea::PostIncrement(y), Ea::AbsoluteLong(a)) => Ok([vec![0xF600 | y], long(a, line)?.to_vec()].concat()),
                (Ea::AbsoluteLong(a), Ea::PostIncrement(y)) => Ok([vec![0xF608 | y], long(a, line)?.to_vec()].concat()),
                (Ea::Indirect(y), Ea::AbsoluteLong(a)) => Ok([vec![0xF610 | y], long(a, line)?.to_vec()].concat()),
                (Ea::AbsoluteLong(a), Ea::Indirect(y)) => Ok([vec![0xF618 | y], long(a, line)?.to_vec()].concat()),
                _ => Err(crate::i18n::tr("MOVE16 moves (Ax)+ to (Ay)+, or between (Ay)+ or (Ay) and a long address").into()),
            }
        }
        "cinvl" | "cinvp" | "cpushl" | "cpushp" => {
            needs(cpu, Need::M68040, &bare)?;
            no_size()?;
            let [which, place] = texts.as_slice() else { return wrong() };
            let caches = cache(which)?;
            let Ea::Indirect(r) = operand(place)? else {
                return Err(crate::i18n::tf!("{bare} takes the line's or page's address in an address register in brackets, such as (a0)"));
            };
            let push = if name.starts_with("cpush") { 0x20 } else { 0 };
            let scope = if name.ends_with('l') { 0x08 } else { 0x10 };
            Ok(vec![0xF400 | caches << 6 | push | scope | r])
        }
        "cinva" | "cpusha" => {
            needs(cpu, Need::M68040, &bare)?;
            no_size()?;
            let [which] = texts.as_slice() else { return wrong() };
            let push = if name == "cpusha" { 0x20 } else { 0 };
            Ok(vec![0xF418 | cache(which)? << 6 | push])
        }
        "lpstop" => {
            needs(cpu, Need::M68060OrCpu32, "LPSTOP")?;
            if size.is_some_and(|s| s != "w") {
                return Err(crate::i18n::tr("LPSTOP is .W").into());
            }
            let [value] = texts.as_slice() else { return wrong() };
            let Some(value) = value.trim().strip_prefix('#') else { return wrong() };
            let v = line.eval(value)?;
            if line.checking() && !(-32768..=65535).contains(&v) {
                return Err(crate::i18n::tf!("LPSTOP's new status register is 16 bits, and {v} doesn't fit"));
            }
            Ok(vec![0xF800, 0x01C0, v as u16])
        }
        "bgnd" => {
            needs(cpu, Need::Cpu32, "BGND")?;
            no_size()?;
            if !texts.is_empty() {
                return Err(crate::i18n::tr("BGND takes no operands").into());
            }
            Ok(vec![0x4AFA])
        }
        _ => {
            let Some((_, kind)) = TABLES.iter().find(|(n, _)| *n == name) else { return wrong() };
            needs(cpu, Need::Cpu32, &bare)?;
            let size = match size {
                None | Some("w") => Size::Word,
                Some("b") => Size::Byte,
                Some("l") => Size::Long,
                Some(_) => return Err(crate::i18n::tf!("{bare} is .B, .W or .L")),
            };
            let [table, result] = texts.as_slice() else { return wrong() };
            let Ok(Ea::D(x)) = operand(result) else {
                return Err(crate::i18n::tf!("{bare} puts what it finds in a data register, and {result} isn't one"));
            };
            match operand(table)? {
                // Between two entries in data registers, Dym:Dyn.
                Ea::Pair(m, n) => Ok(vec![0xF800 | m, x << 12 | kind | size.bits() << 6 | n]),
                ea => {
                    let ea = settle(ea, table, line, here + 4, cpu)?;
                    if !ea.is_control() {
                        return Err(crate::i18n::tf!("{bare} looks up a table at an address such as (a0), and {table} isn't one"));
                    }
                    let mut words = vec![0xF800 | ea.bits(), x << 12 | kind | 0x0100 | size.bits() << 6];
                    words.extend(extension(&ea, size, line, here + 4)?);
                    Ok(words)
                }
            }
        }
    }
}
