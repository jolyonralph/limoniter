//! Every x86 instruction Limoniter's own encoder doesn't write by hand (the x87's, the Pentium's, MMX, SSE and
//! later), encoded by iced-x86 from its own tables: each encoding of the mnemonic is tried with the operands
//! given, and the shortest that takes them is kept. Where encodings differ only in their memory's size (FLD
//! DWORD or QWORD), the size has to be said.

use super::{Level, Mem, Op, REGS8, REGS16, REGS32, SEGS};
use crate::asm::Line;
use iced_x86::{Code, Encoder, Instruction, MemoryOperand, OpCodeOperandKind, Register};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Each mnemonic's encodings usable in 16- or 32-bit code, in iced's order.
fn codes() -> &'static HashMap<String, Vec<Code>> {
    static CODES: OnceLock<HashMap<String, Vec<Code>>> = OnceLock::new();
    CODES.get_or_init(|| {
        let mut map: HashMap<String, Vec<Code>> = HashMap::new();
        for code in Code::values() {
            let info = code.op_code();
            if !info.is_instruction() || !(info.is_available_in_mode(16) || info.is_available_in_mode(32)) {
                continue;
            }
            // Branches, and the string instructions' implied operands, are written by hand.
            if info.op_kinds().iter().any(|k| matches!(k, OpCodeOperandKind::br16_1 | OpCodeOperandKind::br32_1 | OpCodeOperandKind::br16_2 | OpCodeOperandKind::br32_4 | OpCodeOperandKind::br64_1 | OpCodeOperandKind::br64_4 | OpCodeOperandKind::brdisp_2 | OpCodeOperandKind::brdisp_4 | OpCodeOperandKind::xbegin_2 | OpCodeOperandKind::xbegin_4 | OpCodeOperandKind::farbr2_2 | OpCodeOperandKind::farbr4_2)) {
                continue;
            }
            map.entry(format!("{:?}", info.mnemonic()).to_ascii_lowercase()).or_default().push(code);
        }
        map
    })
}

/// Every register by its name in lower case.
fn registers() -> &'static HashMap<String, Register> {
    static REGISTERS: OnceLock<HashMap<String, Register>> = OnceLock::new();
    REGISTERS.get_or_init(|| Register::values().filter(|r| *r != Register::None).map(|r| (format!("{r:?}").to_ascii_lowercase(), r)).collect())
}

pub(super) fn register(name: &str) -> Option<Register> {
    registers().get(name).copied()
}

pub(super) fn knows(mnemonic: &str) -> bool {
    codes().contains_key(mnemonic)
}

/// An operand as iced takes it.
#[derive(Clone, Copy)]
enum Arg {
    R(Register),
    I(i32),
    M(MemoryOperand),
}

fn general(list: &[&str; 8], n: u8) -> Register {
    register(list[usize::from(n)]).unwrap_or(Register::None)
}

fn arg(op: &Op, line: &mut Line) -> Result<(Arg, Option<u8>), String> {
    Ok(match op {
        Op::Reg(r) => (Arg::R(general(if r.size == 1 { &REGS8 } else if r.size == 2 { &REGS16 } else { &REGS32 }, r.num)), None),
        Op::Seg(s) => (Arg::R(register(SEGS[usize::from(*s)]).unwrap_or(Register::None)), None),
        Op::Control(n) => (Arg::R(register(&format!("cr{n}")).ok_or_else(|| crate::i18n::tf!("CR{n} isn't a register"))?), None),
        Op::Debug(n) => (Arg::R(register(&format!("dr{n}")).ok_or_else(|| crate::i18n::tf!("DR{n} isn't a register"))?), None),
        Op::Test(n) => (Arg::R(register(&format!("tr{n}")).ok_or_else(|| crate::i18n::tf!("TR{n} isn't a register"))?), None),
        Op::Named(r) => (Arg::R(*r), None),
        Op::Mem(m) => (Arg::M(memory(m, line)?), m.size),
        Op::Imm { text, .. } => {
            let v = line.eval(text)?;
            (Arg::I(if v > i64::from(i32::MAX) { v as u32 as i32 } else { v as i32 }), None)
        }
        Op::Far { .. } => return Err(crate::i18n::tr("a seg:offset pointer only goes with JMP and CALL").into()),
    })
}

fn memory(m: &Mem, line: &mut Line) -> Result<MemoryOperand, String> {
    let reg = |r: Option<super::Reg>| r.map_or(Register::None, |r| general(if r.size == 2 { &REGS16 } else { &REGS32 }, r.num));
    let displacement = match &m.disp {
        Some(d) => line.eval(d)?,
        None => 0,
    };
    // A displacement whose label isn't known yet takes its full size.
    let displ_size = match &m.disp {
        None => 0,
        Some(d) if line.known(d).is_some() => 1,
        Some(_) if m.base.is_some_and(|b| b.size == 2) || m.index.is_some_and(|i| i.size == 2) => 2,
        Some(_) => 4,
    };
    let seg = m.seg.map_or(Register::None, |s| register(SEGS[usize::from(s)]).unwrap_or(Register::None));
    Ok(MemoryOperand::new(reg(m.base), reg(m.index), u32::from(m.scale), displacement, displ_size, false, seg))
}

/// An instruction made from a code and its operands, as iced's constructors take them.
fn build(code: Code, args: &[Arg]) -> Option<Instruction> {
    use Arg::*;
    let made = match *args {
        [] => Ok(Instruction::with(code)),
        [R(a)] => Instruction::with1(code, a),
        [I(a)] => Instruction::with1(code, a),
        [M(a)] => Instruction::with1(code, a),
        [R(a), R(b)] => Instruction::with2(code, a, b),
        [R(a), I(b)] => Instruction::with2(code, a, b),
        [R(a), M(b)] => Instruction::with2(code, a, b),
        [I(a), R(b)] => Instruction::with2(code, a, b),
        [I(a), I(b)] => Instruction::with2(code, a, b),
        [M(a), R(b)] => Instruction::with2(code, a, b),
        [M(a), I(b)] => Instruction::with2(code, a, b),
        [R(a), R(b), R(c)] => Instruction::with3(code, a, b, c),
        [R(a), R(b), I(c)] => Instruction::with3(code, a, b, c),
        [R(a), R(b), M(c)] => Instruction::with3(code, a, b, c),
        [R(a), I(b), I(c)] => Instruction::with3(code, a, b, c),
        [R(a), M(b), R(c)] => Instruction::with3(code, a, b, c),
        [R(a), M(b), I(c)] => Instruction::with3(code, a, b, c),
        [M(a), R(b), R(c)] => Instruction::with3(code, a, b, c),
        [M(a), R(b), I(c)] => Instruction::with3(code, a, b, c),
        [R(a), R(b), R(c), R(d)] => Instruction::with4(code, a, b, c, d),
        [R(a), R(b), R(c), I(d)] => Instruction::with4(code, a, b, c, d),
        [R(a), R(b), M(c), R(d)] => Instruction::with4(code, a, b, c, d),
        [R(a), R(b), M(c), I(d)] => Instruction::with4(code, a, b, c, d),
        [R(a), R(b), R(c), M(d)] => Instruction::with4(code, a, b, c, d),
        _ => return None,
    };
    made.ok()
}

/// The processor an instruction needs, from what CPUID says it needs.
fn level_of(code: Code) -> Level {
    code.cpuid_features()
        .iter()
        .map(|f| {
            let name = format!("{f:?}");
            match name.as_str() {
                n if n.starts_with("INTEL8086") || n == "FPU" => Level::I8086,
                "INTEL186" => Level::I186,
                n if n.starts_with("INTEL286") || n.starts_with("FPU287") => Level::I286,
                n if n.starts_with("INTEL386") || n.starts_with("FPU387") || n == "UMOV" => Level::I386,
                n if n.starts_with("INTEL486") || n == "CPUID" => Level::I486,
                "TSC" | "MSR" | "CX8" | "MMX" | "RDPMC" | "SMM" => Level::P5,
                "CMOV" | "FXSR" | "SEP" | "PAE" | "PAUSE" | "MULTIBYTENOP" => Level::P6,
                _ => Level::Any,
            }
        })
        .max()
        .unwrap_or(Level::I8086)
}

/// Encodes an instruction from iced's tables: the shortest encoding that takes the operands given.
pub(super) fn encode(mnemonic: &str, ops: &[Op], line: &mut Line, bits: u8) -> Result<(Vec<u8>, Level), String> {
    let name = mnemonic.to_ascii_uppercase();
    let Some(candidates) = codes().get(mnemonic) else {
        return Err(crate::i18n::tf!("{name} isn't an x86 instruction Limoniter knows"));
    };
    let mut args = Vec::new();
    let mut sizes = Vec::new();
    for op in ops {
        let (a, size) = arg(op, line)?;
        args.push(a);
        sizes.push(size);
    }
    // The x87's shorter forms, as NASM takes them: FADD ST1 is FADD ST0,ST1; FADDP alone is FADDP ST1,ST0.
    let mut tries = vec![args.clone()];
    if mnemonic.starts_with('f') {
        if let [Arg::R(r)] = args[..]
            && (Register::ST0..=Register::ST7).contains(&r)
        {
            tries.push(vec![Arg::R(Register::ST0), Arg::R(r)]);
            tries.push(vec![Arg::R(r), Arg::R(Register::ST0)]);
        }
        if args.is_empty() {
            tries.push(vec![Arg::R(Register::ST1), Arg::R(Register::ST0)]);
            tries.push(vec![Arg::R(Register::ST0), Arg::R(Register::ST1)]);
            tries.push(vec![Arg::R(Register::ST1)]);
        }
    }
    let given = sizes.iter().flatten().next().copied();
    let mut found: Vec<(Vec<u8>, Code, usize)> = Vec::new();
    for args in &tries {
        for &code in candidates {
            let info = code.op_code();
            if info.op_count() as usize != args.len() || !info.is_available_in_mode(u32::from(bits)) {
                continue;
            }
            let Some(instruction) = build(code, args) else { continue };
            let memory = instruction.memory_size().size();
            if let Some(size) = given
                && memory != 0
                && memory != usize::from(size)
            {
                continue;
            }
            let mut encoder = Encoder::new(u32::from(bits));
            if encoder.encode(&instruction, line.here() as u64).is_ok() {
                found.push((encoder.take_buffer(), code, memory));
            }
        }
        if !found.is_empty() {
            break;
        }
    }
    if found.is_empty() {
        return Err(crate::i18n::tf!("{name} can't take those operands"));
    }
    // Memory with no size, where the encodings that take it differ in size.
    if given.is_none() && args.iter().any(|a| matches!(a, Arg::M(_))) {
        let mut sizes: Vec<usize> = found.iter().map(|(_, _, m)| *m).filter(|m| *m != 0).collect();
        sizes.sort_unstable();
        sizes.dedup();
        if sizes.len() > 1 {
            let words: Vec<String> = sizes.iter().map(|s| size_word(*s)).collect();
            return Err(crate::i18n::tf!("{name} needs to know its memory's size: say {}", words.join(", ")));
        }
    }
    let (bytes, code, _) = found.into_iter().min_by_key(|(b, ..)| b.len()).expect("found one");
    Ok((bytes, level_of(code)))
}

fn size_word(size: usize) -> String {
    match size {
        1 => "BYTE".into(),
        2 => "WORD".into(),
        4 => "DWORD".into(),
        6 => "FWORD".into(),
        8 => "QWORD".into(),
        10 => "TWORD".into(),
        16 => "OWORD".into(),
        n => crate::i18n::tf!("{n} bytes"),
    }
}
