//! The F-line's instructions, by the coprocessor number in their first word: 0 the 68030's and 68851's MMU, 1 the
//! FPU, 2 the 68040's and 68060's caches and MMU, 3 MOVE16, 4 the 68060's and CPU32's LPSTOP and the CPU32's table
//! lookups. Only those that Limoniter's assembler and vasm would write again as the same words are read.

use super::{Class, Flow, Op, Reader, Size, ea, fits, fpu, mmu};
use crate::asm::m68k::Processor;
use crate::asm::m68k::later::{CACHES, TABLES};
use crate::asm::m68k::mmu::Kind;

/// An F-line instruction, from its opcode word `op` (already read): its mnemonic and operands.
pub(super) fn decode(r: &mut Reader, op: u16, flow: &mut Flow) -> Option<(String, Vec<Op>)> {
    let late = matches!(r.cpu, Processor::M68040 | Processor::M68060);
    match op >> 9 & 7 {
        0 => mmu::decode(r, op, flow),
        1 if r.fpu => fpu::decode(r, op, flow),
        2 if late && op & 0xFF00 == 0xF400 => caches(op),
        2 if late => mmu::decode_late(r, op),
        3 if late => move16(r, op),
        4 if matches!(r.cpu, Processor::M68060 | Processor::Cpu32) => cpu32(r, op),
        _ => None,
    }
}

/// CINV and CPUSH: which caches, and a line's or page's address, or all of them.
fn caches(op: u16) -> Option<(String, Vec<Op>)> {
    let cache = Op::Name(CACHES[usize::from(op >> 6 & 3)].to_string());
    let name = if op & 0x20 != 0 { "cpush" } else { "cinv" };
    let reg = (op & 7) as u8;
    match op >> 3 & 3 {
        1 => Some((format!("{name}l"), vec![cache, Op::Ind(reg)])),
        2 => Some((format!("{name}p"), vec![cache, Op::Ind(reg)])),
        3 if reg == 0 => Some((format!("{name}a"), vec![cache])),
        _ => None,
    }
}

fn move16(r: &mut Reader, op: u16) -> Option<(String, Vec<Op>)> {
    let reg = (op & 7) as u8;
    if op & 0xFFF8 == 0xF620 {
        let w = r.next().filter(|w| w & 0x8FFF == 0x8000)?;
        return Some(("move16".into(), vec![Op::Post(reg), Op::Post((w >> 12 & 7) as u8)]));
    }
    if op & 0xFFE0 != 0xF600 {
        return None;
    }
    let at = r.here();
    let address = Op::AbsL(r.long()?, at);
    let ops = match op >> 3 & 3 {
        0 => vec![Op::Post(reg), address],
        1 => vec![address, Op::Post(reg)],
        2 => vec![Op::Ind(reg), address],
        _ => vec![address, Op::Ind(reg)],
    };
    Some(("move16".into(), ops))
}

/// LPSTOP (the 68060's and CPU32's), and the CPU32's table lookups.
fn cpu32(r: &mut Reader, op: u16) -> Option<(String, Vec<Op>)> {
    if op & 0xFFC0 != 0xF800 {
        return None;
    }
    let w = r.next()?;
    if op == 0xF800 && w == 0x01C0 {
        let at = r.here();
        return Some(("lpstop".into(), vec![Op::Imm(u32::from(r.next()?), Size::W, at)]));
    }
    if r.cpu != Processor::Cpu32 || w & 0x8200 != 0 {
        return None;
    }
    let size = Size::from_bits(w >> 6 & 3)?;
    let (name, _) = TABLES.iter().find(|(_, bits)| *bits == w & 0x0C00)?;
    let named = format!("{name}{}", size.suffix());
    let result = Op::D((w >> 12 & 7) as u8);
    if w & 0x0100 != 0 {
        if w & 0x003F != 0 {
            return None;
        }
        let table = ea(r, op >> 3 & 7, op & 7, size)?;
        return fits(&table, Class::Control).then(|| (named, vec![table, result]));
    }
    if op & 0x38 != 0 || w & 0x0038 != 0 {
        return None;
    }
    Some((named, vec![Op::Pair((op & 7) as u8, (w & 7) as u8), result]))
}

/// Whether the reader's MMU is one of these.
pub(super) fn has(r: &Reader, kinds: &[Kind]) -> bool {
    r.mmu.is_some_and(|k| kinds.contains(&k))
}
