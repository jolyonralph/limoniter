//! The 68881's and 68882's instructions read back (and the 68040's and 68060's own FPU's, which are the same
//! here): only those that Limoniter's assembler and vasm would write again as the same words, so anything else
//! stays data.

use super::{Class, Flow, Op, Reader, Size, ea, fits};
use crate::asm::m68k::float::Format;
use crate::asm::m68k::Processor;
use crate::asm::m68k::fpu::{CONDITIONS, CONTROL, DYADIC, FTST, MONADIC, ROUNDED};

/// An FPU instruction, from its opcode word `op` (its first word, already read): its mnemonic and operands.
pub(super) fn decode(r: &mut Reader, op: u16, flow: &mut Flow) -> Option<(String, Vec<Op>)> {
    // Only coprocessor number 1, the FPU's.
    if op >> 9 & 7 != 1 {
        return None;
    }
    let (mode, reg) = (op >> 3 & 7, op & 7);
    match op >> 6 & 7 {
        0 => general(r, op, mode, reg),
        1 => {
            let w = r.next()?;
            if w & 0xFFC0 != 0 || w >= 32 {
                return None;
            }
            let cc = CONDITIONS[usize::from(w)];
            match (mode, reg) {
                (1, _) => {
                    let at = r.here();
                    let d = r.next()? as i16;
                    *flow = Flow::Branch;
                    Some((format!("fdb{cc}"), vec![Op::D(reg as u8), Op::Target(at.wrapping_add(d as i32 as u32))]))
                }
                (7, 2) => {
                    let at = r.here();
                    Some((format!("ftrap{cc}.w"), vec![Op::Imm(u32::from(r.next()?), Size::W, at)]))
                }
                (7, 3) => {
                    let at = r.here();
                    Some((format!("ftrap{cc}.l"), vec![Op::Imm(r.long()?, Size::L, at)]))
                }
                (7, 4) => Some((format!("ftrap{cc}"), Vec::new())),
                _ => {
                    let place = ea(r, mode, reg, Size::B)?;
                    fits(&place, Class::DataAlterable).then(|| (format!("fs{cc}"), vec![place]))
                }
            }
        }
        kind @ (2 | 3) => {
            let cc = op & 0x3F;
            if cc >= 32 {
                return None;
            }
            let at = r.here();
            let long = kind == 3;
            let d = if long { r.long()? as i32 } else { i32::from(r.next()? as i16) };
            // FBF.W to the next word does nothing: it's FNOP.
            if cc == 0 && !long && d == 0 {
                return Some(("fnop".into(), Vec::new()));
            }
            *flow = if cc == 15 { Flow::Jump } else { Flow::Branch };
            Some((format!("fb{}{}", CONDITIONS[usize::from(cc)], if long { ".l" } else { ".w" }), vec![Op::Target(at.wrapping_add(d as u32))]))
        }
        4 => {
            let place = ea(r, mode, reg, Size::L)?;
            (matches!(place, Op::Pre(_)) || fits(&place, Class::ControlAlterable)).then(|| ("fsave".into(), vec![place]))
        }
        5 => {
            let place = ea(r, mode, reg, Size::L)?;
            (matches!(place, Op::Post(_)) || fits(&place, Class::Control)).then(|| ("frestore".into(), vec![place]))
        }
        _ => None,
    }
}

/// An immediate in a format, or an address.
fn source(r: &mut Reader, mode: u16, reg: u16, format: Format) -> Option<Op> {
    if mode == 7 && reg == 4 && format.is_float() {
        let mut bytes = Vec::new();
        for _ in 0..format.bytes() / 2 {
            bytes.extend(r.next()?.to_be_bytes());
        }
        return Some(Op::Float(format, bytes));
    }
    let size = match format {
        Format::Byte => Size::B,
        Format::Word => Size::W,
        _ => Size::L,
    };
    let place = ea(r, mode, reg, size)?;
    // A data register only holds what fits in 32 bits.
    let fits_data = match place {
        Op::D(_) => matches!(format, Format::Byte | Format::Word | Format::Long | Format::Single),
        Op::A(_) => false,
        _ => true,
    };
    fits_data.then_some(place)
}

fn named(name: &str, format: Format) -> String {
    format!("{name}.{}", format.letter())
}

/// The general instructions: a command word after the opcode, saying what's done between which.
fn general(r: &mut Reader, op: u16, mode: u16, reg: u16) -> Option<(String, Vec<Op>)> {
    let w = r.next()?;
    let (from, to, opmode) = (w >> 10 & 7, w >> 7 & 7, w & 0x7F);
    let fp = |n: u16| Op::Fp(n as u8);
    // The 68040's and 68060's own rounding forms (FSADD, FDMOVE) only for them.
    let rounded: &[(&str, u16)] = if matches!(r.cpu, Processor::M68040 | Processor::M68060) { &ROUNDED } else { &[] };
    let calculation = |opmode: u16| MONADIC.iter().chain(&DYADIC).chain(rounded).find(|(_, m)| *m == opmode).map(|(n, _)| *n);
    match w >> 13 {
        // Between FP registers.
        0 => {
            if op & 0x3F != 0 {
                return None;
            }
            if let Some(name) = calculation(opmode) {
                return Some((format!("{name}.x"), vec![fp(from), fp(to)]));
            }
            match opmode {
                0x30..=0x37 => Some(("fsincos.x".into(), vec![fp(from), Op::FpPair((opmode & 7) as u8, to as u8)])),
                FTST if to == 0 => Some(("ftst.x".into(), vec![fp(from)])),
                _ => None,
            }
        }
        // From memory, a data register or an immediate, into an FP register; or one of FMOVECR's constants.
        2 => {
            if from == 7 {
                if op & 0x3F != 0 || w & 0xFC00 != 0x5C00 {
                    return None;
                }
                return Some(("fmovecr.x".into(), vec![Op::Quick(i32::from(opmode)), fp(to)]));
            }
            let format = Format::from_code(from)?;
            if let Some(name) = calculation(opmode) {
                let place = source(r, mode, reg, format)?;
                return Some((named(name, format), vec![place, fp(to)]));
            }
            match opmode {
                0x30..=0x37 => {
                    let place = source(r, mode, reg, format)?;
                    Some((named("fsincos", format), vec![place, Op::FpPair((opmode & 7) as u8, to as u8)]))
                }
                FTST if to == 0 => Some((named("ftst", format), vec![source(r, mode, reg, format)?])),
                _ => None,
            }
        }
        // FMOVE out of an FP register; FMOVE.P with its k-factor.
        3 => {
            let k = w & 0x7F;
            let (format, k_factor) = match from {
                3 => (Format::Packed, (k != 0).then(|| format!("{{#{}}}", (k << 9) as i16 >> 9))),
                7 if k & 0x0F == 0 => (Format::Packed, Some(format!("{{d{}}}", k >> 4))),
                _ if k == 0 => (Format::from_code(from)?, None),
                _ => return None,
            };
            let size = match format {
                Format::Byte => Size::B,
                Format::Word => Size::W,
                _ => Size::L,
            };
            let place = ea(r, mode, reg, size)?;
            let fits_data = match place {
                Op::D(_) => matches!(format, Format::Byte | Format::Word | Format::Long | Format::Single),
                _ => fits(&place, Class::DataAlterable),
            };
            if !fits_data {
                return None;
            }
            let place = match k_factor {
                Some(k) => Op::Field(Box::new(place), k),
                None => place,
            };
            Some((named("fmove", format), vec![fp(to), place]))
        }
        // Into control registers, and out of them.
        kind @ (4 | 5) => {
            let select = w >> 10 & 7;
            if select == 0 || w & 0x03FF != 0 {
                return None;
            }
            let one = select.count_ones() == 1;
            let place = if kind == 4 && mode == 7 && reg == 4 {
                let at = r.here();
                if one {
                    Op::Imm(r.long()?, Size::L, at)
                } else {
                    let mut bytes = Vec::new();
                    for _ in 0..2 * select.count_ones() {
                        bytes.extend(r.next()?.to_be_bytes());
                    }
                    Op::Raw(bytes)
                }
            } else {
                ea(r, mode, reg, Size::L)?
            };
            let fits_it = match place {
                Op::D(_) => one,
                Op::A(_) => select == 1,
                Op::Imm(..) | Op::Raw(_) => kind == 4,
                _ if kind == 4 => fits(&place, Class::Data),
                _ => fits(&place, Class::MemoryAlterable),
            };
            if !fits_it {
                return None;
            }
            let registers = if one { Op::Control(CONTROL.iter().find(|(_, bit)| *bit == select).map(|(n, _)| *n)?) } else { Op::FpControls(select as u8) };
            let name = if one { "fmove.l" } else { "fmovem.l" };
            Some((name.into(), if kind == 4 { vec![place, registers] } else { vec![registers, place] }))
        }
        // FMOVEM of FP registers: to -(An) the list runs FP7 to FP0 from bit 7, elsewhere FP0 to FP7; or a data
        // register holds it.
        kind @ (6 | 7) => {
            let list_mode = w >> 11 & 3;
            if w & 0x0700 != 0 {
                return None;
            }
            let dynamic = list_mode & 1 == 1;
            if dynamic && w & 0x8F != 0 {
                return None;
            }
            let pre = list_mode & 2 == 0;
            let place = ea(r, mode, reg, Size::L)?;
            let fits_it = match (kind, pre) {
                (6, false) => matches!(place, Op::Post(_)) || fits(&place, Class::Control),
                (7, true) => matches!(place, Op::Pre(_)),
                (7, false) => fits(&place, Class::ControlAlterable),
                _ => false,
            };
            let list = if dynamic {
                Op::D((w >> 4 & 7) as u8)
            } else {
                let mask = w as u8;
                if mask == 0 {
                    return None;
                }
                Op::FpList(if pre { mask } else { mask.reverse_bits() })
            };
            if !fits_it {
                return None;
            }
            Some(("fmovem.x".into(), if kind == 6 { vec![place, list] } else { vec![list, place] }))
        }
        _ => None,
    }
}

/// FP registers as a list: fp0-fp3/fp5.
pub(super) fn list(mask: u8) -> String {
    let mut parts = Vec::new();
    let mut n = 0;
    while n < 8 {
        if mask & (1 << n) == 0 {
            n += 1;
            continue;
        }
        let start = n;
        while n + 1 < 8 && mask & (1 << (n + 1)) != 0 {
            n += 1;
        }
        parts.push(if n == start { format!("fp{start}") } else { format!("fp{start}-fp{n}") });
        n += 1;
    }
    parts.join("/")
}

/// Control registers as a list: fpcr/fpsr/fpiar.
pub(super) fn controls(select: u8) -> String {
    CONTROL.iter().filter(|(_, bit)| u16::from(select) & bit != 0).map(|(n, _)| *n).collect::<Vec<_>>().join("/")
}
