//! The Archimedes' ARM (ARM2 and ARM3) disassembled into Acorn's syntax, which Limoniter's ARM assembler takes
//! back to the same bytes: branches and PC-relative loads to labels, RISC OS SWIs by name, and anything that isn't
//! an instruction, or that's written in a way the assembler wouldn't choose, as DCD with what it would be beside it.

use crate::asm::arm::{CONDITIONS, DATA, SHIFTS, encode_immediate, swi_name};
use std::collections::BTreeSet;

fn cond(word: u32) -> &'static str {
    match word >> 28 {
        14 => "",
        n => CONDITIONS[n as usize],
    }
}

fn r(n: u32) -> String {
    match n {
        13 => "sp".into(),
        14 => "lr".into(),
        15 => "pc".into(),
        n => format!("r{n}"),
    }
}

fn hex(v: u32) -> String {
    if v < 10 { v.to_string() } else { format!("&{v:X}") }
}

/// Operand 2 as written: a register (shifted perhaps), or an immediate.
fn operand2(word: u32) -> Option<String> {
    if word & (1 << 25) != 0 {
        let rotate = (word >> 8) & 15;
        let imm = word & 0xFF;
        let value = imm.rotate_right(rotate * 2);
        // The assembler writes a value the one way: an unusual rotation of it is said exactly.
        return Some(if encode_immediate(value) == Some(word & 0xFFF) { format!("#{}", hex(value)) } else { format!("#{},{}", hex(imm), rotate * 2) });
    }
    let rm = r(word & 15);
    let kind = (word >> 5) & 3;
    if word & 0x10 != 0 {
        // By a register: bit 7 must be clear.
        if word & 0x80 != 0 {
            return None;
        }
        return Some(format!("{rm},{} {}", SHIFTS[kind as usize], r((word >> 8) & 15)));
    }
    let amount = (word >> 7) & 31;
    Some(match (kind, amount) {
        (0, 0) => rm,
        (3, 0) => format!("{rm},rrx"),
        (1 | 2, 0) => format!("{rm},{} #32", SHIFTS[kind as usize]),
        _ => format!("{rm},{} #{amount}", SHIFTS[kind as usize]),
    })
}

/// One instruction as Acorn's syntax writes it, with `label` naming an address; None if it isn't one.
pub fn instruction(word: u32, address: u32, label: &dyn Fn(u32) -> String) -> Option<String> {
    let c = cond(word);
    if word >> 28 == 15 {
        // NV: never runs, and later ARMs use it for other things.
        return None;
    }
    // MUL and MLA.
    if word & 0x0FC0_00F0 == 0x0000_0090 {
        let (rd, rn, rs, rm) = ((word >> 16) & 15, (word >> 12) & 15, (word >> 8) & 15, word & 15);
        let s = if word & (1 << 20) != 0 { "s" } else { "" };
        return Some(if word & (1 << 21) != 0 { format!("mla{c}{s} {},{},{},{}", r(rd), r(rm), r(rs), r(rn)) } else if rn == 0 { format!("mul{c}{s} {},{},{}", r(rd), r(rm), r(rs)) } else { return None });
    }
    // SWP and SWPB (the ARM3's).
    if word & 0x0FB0_0FF0 == 0x0100_0090 {
        let b = if word & (1 << 22) != 0 { "b" } else { "" };
        return Some(format!("swp{c}{b} {},{},[{}]", r((word >> 12) & 15), r(word & 15), r((word >> 16) & 15)));
    }
    match (word >> 25) & 7 {
        0 | 1 => {
            // The rest of 000 with bits 7 and 4 set are later ARMs' (halfword transfers): not the ARM2's.
            if word & (1 << 25) == 0 && word & 0x90 == 0x90 {
                return None;
            }
            let opcode = (word >> 21) & 15;
            let s = word & (1 << 20) != 0;
            let (rn, rd) = ((word >> 16) & 15, (word >> 12) & 15);
            let op2 = operand2(word)?;
            let name = DATA[opcode as usize];
            Some(match opcode {
                8..=11 => {
                    if !s {
                        return None;
                    }
                    // TEQP and its kin write the PSR, with Rd as R15; otherwise Rd should be 0.
                    let p = match rd {
                        15 => "p",
                        0 => "",
                        _ => return None,
                    };
                    format!("{name}{c}{p} {},{op2}", r(rn))
                }
                13 | 15 => {
                    if rn != 0 {
                        return None;
                    }
                    format!("{name}{c}{} {},{op2}", if s { "s" } else { "" }, r(rd))
                }
                _ => {
                    // ADD and SUB from PC with an immediate: ADR, if the label's where the value points.
                    format!("{name}{c}{} {},{},{op2}", if s { "s" } else { "" }, r(rd), r(rn))
                }
            })
        }
        2 | 3 => {
            let register_offset = word & (1 << 25) != 0;
            if register_offset && word & 0x10 != 0 {
                return None;
            }
            let (p, u, b, w, l) = (word & (1 << 24) != 0, word & (1 << 23) != 0, word & (1 << 22) != 0, word & (1 << 21) != 0, word & (1 << 20) != 0);
            let (rn, rd) = ((word >> 16) & 15, (word >> 12) & 15);
            let name = if l { "ldr" } else { "str" };
            let t = if !p && w { "t" } else { "" };
            let bb = if b { "b" } else { "" };
            let sign = if u { "" } else { "-" };
            let offset = if register_offset {
                let shifted = operand2(word & !(1 << 25))?;
                format!("{sign}{shifted}")
            } else {
                let v = word & 0xFFF;
                // A label, from PC without writing back.
                if rn == 15 && p && !w && (v != 0 || u) {
                    let target = if u { address.wrapping_add(8).wrapping_add(v) } else { address.wrapping_add(8).wrapping_sub(v) };
                    return Some(format!("{name}{c}{bb} {},{}", r(rd), label(target)));
                }
                match (v, u) {
                    (0, true) => String::new(),
                    // An offset of minus nothing: no assembler writes it.
                    (0, false) => return None,
                    _ => format!("#{sign}{}", hex(v)),
                }
            };
            Some(match (p, offset.is_empty()) {
                (true, true) => format!("{name}{c}{bb} {},[{}]{}", r(rd), r(rn), if w { "!" } else { "" }),
                (true, false) => format!("{name}{c}{bb} {},[{},{offset}]{}", r(rd), r(rn), if w { "!" } else { "" }),
                (false, _) => {
                    let offset = if offset.is_empty() { "#0".into() } else { offset };
                    format!("{name}{c}{bb}{t} {},[{}],{offset}", r(rd), r(rn))
                }
            })
        }
        4 => {
            let (p, u, s, w, l) = (word & (1 << 24) != 0, word & (1 << 23) != 0, word & (1 << 22) != 0, word & (1 << 21) != 0, word & (1 << 20) != 0);
            let list = word & 0xFFFF;
            if list == 0 {
                return None;
            }
            let rn = (word >> 16) & 15;
            // The stack's names for R13, the plain ones for anything else.
            let mode = match (rn == 13, l, p, u) {
                (true, true, false, true) => "fd",
                (true, true, true, true) => "ed",
                (true, true, false, false) => "fa",
                (true, true, true, false) => "ea",
                (true, false, true, false) => "fd",
                (true, false, false, false) => "ed",
                (true, false, true, true) => "fa",
                (true, false, false, true) => "ea",
                (false, _, false, true) => "ia",
                (false, _, true, true) => "ib",
                (false, _, false, false) => "da",
                (false, _, true, false) => "db",
            };
            let mut names = Vec::new();
            let mut n = 0;
            while n < 16 {
                if list & (1 << n) != 0 {
                    let mut end = n;
                    while end + 1 < 16 && list & (1 << (end + 1)) != 0 {
                        end += 1;
                    }
                    names.push(if end >= n + 2 { format!("{}-{}", r(n), r(end)) } else if end == n + 1 { format!("{},{}", r(n), r(end)) } else { r(n) });
                    n = end + 1;
                } else {
                    n += 1;
                }
            }
            Some(format!("{}{c}{mode} {}{},{{{}}}{}", if l { "ldm" } else { "stm" }, r(rn), if w { "!" } else { "" }, names.join(","), if s { "^" } else { "" }))
        }
        5 => {
            let offset = ((word & 0x00FF_FFFF) << 8) as i32 >> 6;
            // 26-bit addresses, as the ARM2 and ARM3 have them: a branch wraps round.
            let target = address.wrapping_add(8).wrapping_add(offset as u32) & 0x03FF_FFFF;
            Some(format!("{}{c} {}", if word & (1 << 24) != 0 { "bl" } else { "b" }, label(target)))
        }
        6 => {
            let (p, u, n, w, l) = (word & (1 << 24) != 0, word & (1 << 23) != 0, word & (1 << 22) != 0, word & (1 << 21) != 0, word & (1 << 20) != 0);
            if !p && !w {
                return None;
            }
            let (rn, crd, cp) = ((word >> 16) & 15, (word >> 12) & 15, (word >> 8) & 15);
            let v = (word & 0xFF) * 4;
            if v == 0 && !u {
                return None;
            }
            let sign = if u { "" } else { "-" };
            let name = format!("{}{c}{}", if l { "ldc" } else { "stc" }, if n { "l" } else { "" });
            Some(if p { format!("{name} p{cp},c{crd},[{},#{sign}{}]{}", r(rn), hex(v), if w { "!" } else { "" }) } else { format!("{name} p{cp},c{crd},[{}],#{sign}{}", r(rn), hex(v)) })
        }
        _ => {
            if word & (1 << 24) != 0 {
                let number = word & 0x00FF_FFFF;
                return Some(match swi_name(number) {
                    Some(name) => format!("swi{c} \"{name}\""),
                    None => format!("swi{c} &{number:X}"),
                });
            }
            let (crn, rd, cp, crm) = ((word >> 16) & 15, (word >> 12) & 15, (word >> 8) & 15, word & 15);
            let op2 = (word >> 5) & 7;
            if word & 0x10 != 0 {
                let op1 = (word >> 21) & 7;
                let name = if word & (1 << 20) != 0 { "mrc" } else { "mcr" };
                Some(format!("{name}{c} p{cp},{op1},{},c{crn},c{crm},{op2}", r(rd)))
            } else {
                let op1 = (word >> 20) & 15;
                Some(format!("cdp{c} p{cp},{op1},c{rd},c{crn},c{crm},{op2}"))
            }
        }
    }
}

/// Code at an address, disassembled: labels where branches and loads go, each instruction with its address and
/// bytes beside it, and what isn't an instruction as DCD.
pub fn disassemble(bytes: &[u8], origin: u32) -> String {
    let words: Vec<u32> = bytes.chunks(4).map(|c| { let mut b = [0u8; 4]; b[..c.len()].copy_from_slice(c); u32::from_le_bytes(b) }).collect();
    let end = origin.wrapping_add(words.len() as u32 * 4);
    let inside = |a: u32| a >= origin && a < end && a.is_multiple_of(4);
    // Where branches and PC-relative loads go, inside the code: those get labels.
    let found = std::cell::RefCell::new(BTreeSet::new());
    for (i, &word) in words.iter().enumerate() {
        let address = origin + i as u32 * 4;
        let _ = instruction(word, address, &|t| {
            if inside(t) {
                found.borrow_mut().insert(t);
            }
            String::new()
        });
    }
    let targets = found.into_inner();
    let name = |a: u32| format!("l{a:08X}");
    let label = |t: u32| if inside(t) { name(t) } else { format!("&{t:X}") };
    let mut out = crate::i18n::tf!("; ARM code (the Archimedes' ARM2 and ARM3), disassembled by Limoniter: it assembles back to the same bytes.\n");
    out.push_str(&format!("        org &{origin:X}\n\n"));
    for (i, &word) in words.iter().enumerate() {
        let address = origin + i as u32 * 4;
        if targets.contains(&address) {
            out.push_str(&format!("{}\n", name(address)));
        }
        let text = instruction(word, address, &label).unwrap_or_else(|| format!("dcd &{word:08X}"));
        out.push_str(&format!("        {text:<40}; {address:08X}  {word:08X}\n"));
    }
    out
}

#[cfg(test)]
mod tests;
