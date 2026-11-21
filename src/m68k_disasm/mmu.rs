//! The MMUs' instructions read back: the 68030's and the 68851's (coprocessor 0's, with a command word after the
//! opcode), and the 68040's and 68060's (one word each). Only those that Limoniter's assembler and vasm would write
//! again as the same words are read, so anything else stays data.

use super::later::has;
use super::{Class, Flow, Op, Reader, Size, ea, fits};
use crate::asm::m68k::mmu::{CONDITIONS, Kind, register_name};

/// A function code as written: SFC, DFC, Dn, or a number (0 to 7 on the 68030, 0 to 15 on the 68851).
fn function_code(w: u16, kind: Kind) -> Option<Op> {
    let fc = w & 0x1F;
    Some(match fc {
        0 => Op::Name("sfc".into()),
        1 => Op::Name("dfc".into()),
        0x08..=0x0F => Op::D((fc & 7) as u8),
        0x10..=0x17 => Op::Quick(i32::from(fc & 7)),
        0x18..=0x1F if kind == Kind::M68851 => Op::Quick(i32::from(fc & 15)),
        _ => return None,
    })
}

/// Coprocessor 0's instructions: the 68030's MMU's, or the 68851's. `op` is the opcode word, already read.
pub(super) fn decode(r: &mut Reader, op: u16, flow: &mut Flow) -> Option<(String, Vec<Op>)> {
    let kind = r.mmu.filter(|k| matches!(k, Kind::M68030 | Kind::M68851))?;
    let pmmu = kind == Kind::M68851;
    let (mode, reg) = (op >> 3 & 7, op & 7);
    match op >> 6 & 7 {
        0 => general(r, op, kind),
        1 if pmmu => {
            let w = r.next().filter(|w| *w < 16)?;
            let cc = CONDITIONS[usize::from(w)];
            match (mode, reg) {
                (1, _) => {
                    let at = r.here();
                    let d = r.next()? as i16;
                    *flow = Flow::Branch;
                    Some((format!("pdb{cc}"), vec![Op::D(reg as u8), Op::Target(at.wrapping_add(d as i32 as u32))]))
                }
                (7, 2) => {
                    let at = r.here();
                    Some((format!("ptrap{cc}.w"), vec![Op::Imm(u32::from(r.next()?), Size::W, at)]))
                }
                (7, 3) => {
                    let at = r.here();
                    Some((format!("ptrap{cc}.l"), vec![Op::Imm(r.long()?, Size::L, at)]))
                }
                (7, 4) => Some((format!("ptrap{cc}"), Vec::new())),
                _ => {
                    let place = ea(r, mode, reg, Size::B)?;
                    fits(&place, Class::DataAlterable).then(|| (format!("ps{cc}"), vec![place]))
                }
            }
        }
        branch @ (2 | 3) if pmmu => {
            let cc = op & 0x3F;
            if cc >= 16 {
                return None;
            }
            let at = r.here();
            let long = branch == 3;
            let d = if long { r.long()? as i32 } else { i32::from(r.next()? as i16) };
            *flow = Flow::Branch;
            Some((format!("pb{}{}", CONDITIONS[usize::from(cc)], if long { ".l" } else { ".w" }), vec![Op::Target(at.wrapping_add(d as u32))]))
        }
        4 if pmmu => {
            let place = ea(r, mode, reg, Size::L)?;
            (matches!(place, Op::Pre(_)) || fits(&place, Class::ControlAlterable)).then(|| ("psave".into(), vec![place]))
        }
        5 if pmmu => {
            let place = ea(r, mode, reg, Size::L)?;
            (matches!(place, Op::Post(_)) || fits(&place, Class::Control)).then(|| ("prestore".into(), vec![place]))
        }
        _ => None,
    }
}

/// A root pointer's 64 bits, as an immediate.
fn quad(r: &mut Reader) -> Option<Op> {
    let mut bytes = Vec::new();
    for _ in 0..4 {
        bytes.extend(r.next()?.to_be_bytes());
    }
    Some(Op::Raw(bytes))
}

/// The instructions with a command word after the opcode: PMOVE, PFLUSH, PLOAD, PTEST, PVALID and PFLUSHR.
fn general(r: &mut Reader, op: u16, kind: Kind) -> Option<(String, Vec<Op>)> {
    let pmmu = kind == Kind::M68851;
    let (mode, reg) = (op >> 3 & 7, op & 7);
    let w = r.next()?;
    let control_alterable = |place: &Op| fits(place, Class::ControlAlterable);
    match w >> 13 {
        // The 68030's transparent translation registers.
        0 if !pmmu => {
            let preg = w >> 10 & 7;
            let (into, fd) = (w & 0x200 == 0, w & 0x100 != 0);
            if w & 0xE0FF != 0 || !(2..=3).contains(&preg) || (fd && !into) {
                return None;
            }
            let place = ea(r, mode, reg, Size::L)?;
            if !control_alterable(&place) {
                return None;
            }
            pmove(if fd { "pmovefd" } else { "pmove" }, register_name(w)?, place, into)
        }
        1 => {
            let no_address = op & 0x3F == 0;
            if w == 0x2400 && no_address {
                return Some(("pflusha".into(), Vec::new()));
            }
            // PLOAD.
            if w & 0xFDE0 == 0x2000 {
                let fc = function_code(w, kind)?;
                let place = ea(r, mode, reg, Size::L)?;
                return control_alterable(&place).then(|| ((if w & 0x200 != 0 { "ploadr" } else { "ploadw" }).to_string(), vec![fc, place]));
            }
            // The 68851's PVALID.
            if pmmu && (w == 0x2800 || w & 0xFFF8 == 0x2C00) {
                let with = if w == 0x2800 { Op::Name("val".into()) } else { Op::A((w & 7) as u8) };
                let place = ea(r, mode, reg, Size::L)?;
                return control_alterable(&place).then(|| ("pvalid".to_string(), vec![with, place]));
            }
            // PFLUSH, and the 68851's PFLUSHS: by function code and mask, and at an address.
            if w & 0xF200 != 0x3000 {
                return None;
            }
            let shared = w & 0x0400 != 0;
            let at_address = w & 0x0800 != 0;
            if (shared && !pmmu) || (!pmmu && w & 0x0100 != 0) {
                return None;
            }
            let fc = function_code(w, kind)?;
            let mask = Op::Quick(i32::from(w >> 5 & 15));
            let name = if shared { "pflushs" } else { "pflush" };
            if !at_address {
                return no_address.then(|| (name.to_string(), vec![fc, mask]));
            }
            let place = ea(r, mode, reg, Size::L)?;
            control_alterable(&place).then(|| (name.to_string(), vec![fc, mask, place]))
        }
        // TC and the root pointers, and the 68851's others.
        2 => {
            let preg = w >> 10 & 7;
            let (into, fd) = (w & 0x200 == 0, w & 0x100 != 0);
            if w & 0x00FF != 0 || (fd && (pmmu || !into)) || (!pmmu && preg == 1) || (!pmmu && preg >= 4) {
                return None;
            }
            let quad_sized = (1..=3).contains(&preg);
            let size = match preg {
                0 => Size::L,
                4..=6 => Size::B,
                _ => Size::W,
            };
            let place = if pmmu && quad_sized && into && mode == 7 && reg == 4 { quad(r)? } else { ea(r, mode, reg, size)? };
            let fits_it = match (pmmu, quad_sized, into) {
                (false, _, _) => control_alterable(&place),
                (true, true, true) => matches!(place, Op::Raw(_)) || (!matches!(place, Op::D(_) | Op::A(_) | Op::Imm(..))),
                (true, true, false) => control_alterable(&place),
                (true, false, true) => true,
                (true, false, false) => fits(&place, Class::Alterable),
            };
            if !fits_it {
                return None;
            }
            pmove(if fd { "pmovefd" } else { "pmove" }, register_name(w)?, place, into)
        }
        // MMUSR, and the 68851's PCSR, BAD and BAC registers.
        3 => {
            let into = w & 0x200 == 0;
            let preg = w >> 10 & 7;
            let fits_number = if pmmu {
                w & 0x01E3 == 0 && match preg {
                    0 | 4 | 5 => preg != 0 || w & 0x1C == 0,
                    1 => !into && w & 0x1C == 0,
                    _ => false,
                }
            } else {
                w & 0x1DFF == 0
            };
            if !fits_number {
                return None;
            }
            let place = ea(r, mode, reg, Size::W)?;
            let fits_it = match (pmmu, into) {
                (false, _) => control_alterable(&place),
                (true, true) => true,
                (true, false) => fits(&place, Class::Alterable),
            };
            if !fits_it {
                return None;
            }
            pmove("pmove", register_name(w)?, place, into)
        }
        // PTEST: the level to search to, and the address register for the last descriptor's address.
        4 => {
            let fc = function_code(w, kind)?;
            let level = w >> 10 & 7;
            let with_an = w & 0x100 != 0;
            if (!with_an && w & 0xE0 != 0) || (with_an && level == 0) {
                return None;
            }
            let place = ea(r, mode, reg, Size::L)?;
            if !control_alterable(&place) {
                return None;
            }
            let mut ops = vec![fc, place, Op::Quick(i32::from(level))];
            if with_an {
                ops.push(Op::A((w >> 5 & 7) as u8));
            }
            Some(((if w & 0x200 != 0 { "ptestr" } else { "ptestw" }).to_string(), ops))
        }
        // The 68851's PFLUSHR: a root pointer from memory.
        5 if pmmu && w == 0xA000 => {
            let place = if mode == 7 && reg == 4 { quad(r)? } else { ea(r, mode, reg, Size::L)? };
            (!matches!(place, Op::D(_) | Op::A(_))).then(|| ("pflushr".into(), vec![place]))
        }
        _ => None,
    }
}

/// PMOVE's operands, in the order they go.
fn pmove(name: &str, register: String, place: Op, into: bool) -> Option<(String, Vec<Op>)> {
    let register = Op::Name(register);
    Some((name.to_string(), if into { vec![place, register] } else { vec![register, place] }))
}

/// The 68040's and 68060's own: PFLUSH, PFLUSHN, PFLUSHA and PFLUSHAN; the 68040's PTEST; the 68060's PLPA.
pub(super) fn decode_late(r: &mut Reader, op: u16) -> Option<(String, Vec<Op>)> {
    let reg = (op & 7) as u8;
    let one = |name: &str| Some((name.to_string(), vec![Op::Ind(reg)]));
    match op & 0xFFF8 {
        0xF500 => one("pflushn"),
        0xF508 => one("pflush"),
        0xF510 if reg == 0 => Some(("pflushan".into(), Vec::new())),
        0xF518 if reg == 0 => Some(("pflusha".into(), Vec::new())),
        0xF548 if has(r, &[Kind::M68040]) => one("ptestw"),
        0xF568 if has(r, &[Kind::M68040]) => one("ptestr"),
        0xF588 if has(r, &[Kind::M68060]) => one("plpaw"),
        0xF5C8 if has(r, &[Kind::M68060]) => one("plpar"),
        _ => None,
    }
}
