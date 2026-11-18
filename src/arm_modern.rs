//! Later ARM code, for reading: 32-bit ARM and Thumb (to ARMv7, as phones and the first Raspberry Pis run) and
//! 64-bit AArch64 (the Raspberry Pi 3 on, and Apple's and everyone's since), decoded by yaxpeax-arm and written in
//! GNU's syntax. A Linux program (ELF) is taken from its executable parts; a Raspberry Pi's kernel image
//! (kernel8.img, kernel7.img) from where the Pi loads it. Unlike the Archimedes' code, these aren't assembled again
//! by Limoniter: they're to read.

use yaxpeax_arch::{Decoder, LengthedInstruction, U8Reader};

/// Which instructions the code is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 32-bit ARM, ARMv7 and before.
    Arm32,
    /// Thumb and Thumb-2.
    Thumb,
    /// 64-bit ARM: ARMv8's A64.
    A64,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Arm32, Mode::Thumb, Mode::A64];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Arm32 => "32-bit ARM (ARMv7)",
            Mode::Thumb => "Thumb (ARMv7)",
            Mode::A64 => "64-bit ARM (AArch64)",
        }
    }
}

/// Code found in a file: where each part goes, and what the file is.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub mode: Mode,
    /// Each executable part: where it's loaded, and its bytes.
    pub parts: Vec<(u64, Vec<u8>)>,
    pub entry: Option<u64>,
    pub what: String,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// A file of later ARM code, if it's one: an ELF program for ARM or AArch64, a Raspberry Pi kernel image, or
/// Linux's arm64 Image.
pub fn recognise(name: &str, bytes: &[u8]) -> Option<Program> {
    if bytes.starts_with(b"\x7FELF") {
        return elf(bytes);
    }
    let lower = name.rsplit(['/', '\\']).next().unwrap_or(name).to_ascii_lowercase();
    // Linux's arm64 Image: "ARMd" at 0x38, loaded at its text offset.
    if bytes.get(0x38..0x3C) == Some(b"ARM\x64") {
        let offset = u64_at(bytes, 8).unwrap_or(0x80000);
        return Some(Program { mode: Mode::A64, parts: vec![(offset, bytes.to_vec())], entry: Some(offset), what: crate::i18n::tf!("A Linux kernel for 64-bit ARM (arm64 Image), {} bytes", bytes.len()) });
    }
    // The Raspberry Pi's firmware loads kernel8.img at 0x80000 to run 64-bit, and kernel7.img or kernel.img at
    // 0x8000 to run 32-bit.
    let (mode, at, which) = match lower.as_str() {
        "kernel8.img" | "kernel_2712.img" => (Mode::A64, 0x80000, "64-bit"),
        "kernel7.img" | "kernel7l.img" | "kernel.img" => (Mode::Arm32, 0x8000, "32-bit"),
        _ => return None,
    };
    Some(Program { mode, parts: vec![(at, bytes.to_vec())], entry: Some(at), what: crate::i18n::tf!("A Raspberry Pi kernel image ({which}), loaded at &{at:X}") })
}

/// An ELF program for ARM (machine 40) or AArch64 (183): its loaded parts that can run.
fn elf(bytes: &[u8]) -> Option<Program> {
    let wide = *bytes.get(4)? == 2;
    if *bytes.get(5)? != 1 {
        return None;
    }
    let machine = u16_at(bytes, 0x12)?;
    let mut parts = Vec::new();
    let (entry, ph, phsize, phnum) = if wide {
        (u64_at(bytes, 0x18)?, u64_at(bytes, 0x20)? as usize, usize::from(u16_at(bytes, 0x36)?), usize::from(u16_at(bytes, 0x38)?))
    } else {
        (u64::from(u32_at(bytes, 0x18)?), u32_at(bytes, 0x1C)? as usize, usize::from(u16_at(bytes, 0x2A)?), usize::from(u16_at(bytes, 0x2C)?))
    };
    for n in 0..phnum.min(64) {
        let at = ph + n * phsize;
        let kind = u32_at(bytes, at)?;
        let (flags, offset, address, size) = if wide {
            (u32_at(bytes, at + 4)?, u64_at(bytes, at + 8)? as usize, u64_at(bytes, at + 16)?, u64_at(bytes, at + 32)? as usize)
        } else {
            (u32_at(bytes, at + 24)?, u32_at(bytes, at + 4)? as usize, u64::from(u32_at(bytes, at + 8)?), u32_at(bytes, at + 16)? as usize)
        };
        // PT_LOAD with PF_X.
        if kind == 1 && flags & 1 != 0 {
            let part = bytes.get(offset..offset.checked_add(size)?.min(bytes.len()))?;
            parts.push((address, part.to_vec()));
        }
    }
    let (mode, kind) = match (machine, wide) {
        (183, true) => (Mode::A64, "64-bit ARM (AArch64)"),
        // An odd entry point starts in Thumb.
        (40, false) if entry & 1 == 1 => (Mode::Thumb, "32-bit ARM, starting in Thumb"),
        (40, false) => (Mode::Arm32, "32-bit ARM"),
        _ => return None,
    };
    if parts.is_empty() {
        return None;
    }
    let total: usize = parts.iter().map(|(_, p)| p.len()).sum();
    Some(Program { mode, parts, entry: Some(entry & !1), what: crate::i18n::tf!("A Linux program for {kind} (ELF), {total} bytes of code") })
}

/// One instruction's text and length, or None where the bytes aren't one.
fn decode(bytes: &[u8], mode: Mode) -> Option<(String, usize)> {
    match mode {
        Mode::A64 => {
            let decoder = yaxpeax_arm::armv8::a64::InstDecoder::default();
            let instruction = decoder.decode(&mut U8Reader::new(bytes)).ok()?;
            Some((instruction.to_string(), instruction.len().to_const() as usize))
        }
        Mode::Arm32 | Mode::Thumb => {
            let decoder = if mode == Mode::Thumb { yaxpeax_arm::armv7::InstDecoder::armv7_thumb() } else { yaxpeax_arm::armv7::InstDecoder::armv7() };
            let instruction = decoder.decode(&mut U8Reader::new(bytes)).ok()?;
            Some((instruction.to_string(), instruction.len().to_const() as usize))
        }
    }
}

/// Code disassembled from an address, an instruction a line with its address and bytes; what isn't an instruction
/// as .word (or .hword in Thumb).
pub fn listing(bytes: &[u8], origin: u64, mode: Mode, out: &mut String) {
    let unit = if mode == Mode::Thumb { 2 } else { 4 };
    let mut at = 0;
    while at + unit <= bytes.len() {
        let address = origin + at as u64;
        let (text, length) = match decode(&bytes[at..], mode) {
            Some((text, length)) if length > 0 && at + length <= bytes.len() => (text, length),
            _ => {
                let word = if unit == 2 { format!(".hword 0x{:04x}", u16_at(bytes, at).unwrap_or(0)) } else { format!(".word 0x{:08x}", u32_at(bytes, at).unwrap_or(0)) };
                (word, unit)
            }
        };
        let hex: String = bytes[at..at + length].iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
        out.push_str(&format!("        {text:<44} // {address:08x}  {hex}\n"));
        at += length;
    }
}

/// A program disassembled, each executable part from where it's loaded.
pub fn disassemble(program: &Program) -> String {
    let mut out = crate::i18n::tf!("// {}, disassembled by Limoniter as {}, for reading.\n", program.what, crate::i18n::tr(program.mode.name()));
    if let Some(entry) = program.entry {
        out.push_str(&crate::i18n::tf!("// It starts at {entry:08x}.\n"));
    }
    for (address, bytes) in &program.parts {
        out.push_str(&format!("\n// {address:08x}\n"));
        listing(bytes, *address, program.mode, &mut out);
    }
    out
}

/// Plain code in a mode, from an address.
pub fn raw(bytes: &[u8], origin: u64, mode: Mode) -> Program {
    Program { mode, parts: vec![(origin, bytes.to_vec())], entry: Some(origin), what: crate::i18n::tf!("{} bytes of code", bytes.len()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_later_arm_code_reads_as_itself() {
        let mut out = String::new();
        // AArch64: ORR W20,W20,#4; RET.
        listing(&[0x94, 0x02, 0x1E, 0x32, 0xC0, 0x03, 0x5F, 0xD6], 0x80000, Mode::A64, &mut out);
        assert!(out.contains("orr w20, w20, #0x4") && out.contains("ret"), "{out}");
        // 32-bit ARM: MOV R0,#1; BX LR.
        out.clear();
        listing(&[0x01, 0x00, 0xA0, 0xE3, 0x1E, 0xFF, 0x2F, 0xE1], 0x8000, Mode::Arm32, &mut out);
        assert!(out.contains("mov r0, 0x1") && out.contains("bx lr"), "{out}");
        // Thumb: MOVS R0,#1 (2 bytes); then a 32-bit Thumb-2 BL.
        out.clear();
        listing(&[0x01, 0x20, 0x00, 0xF0, 0x00, 0xF8], 0x8000, Mode::Thumb, &mut out);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(lines[0].contains("movs r0, #0x1") || lines[0].contains("mov"), "{out}");
        assert!(lines[1].contains("bl"), "{out}");
        // Bytes that aren't an instruction are kept as data.
        out.clear();
        listing(&[0xFF, 0xFF, 0xFF, 0xFF], 0, Mode::A64, &mut out);
        assert!(out.contains(".word 0xffffffff"), "{out}");
    }

    #[test]
    fn a_raspberry_pi_kernel_and_an_elf_program_are_recognised() {
        let pi = recognise("boot/kernel8.img", &[0xC0, 0x03, 0x5F, 0xD6]).unwrap();
        assert_eq!((pi.mode, pi.parts[0].0), (Mode::A64, 0x80000));
        assert_eq!(recognise("kernel7.img", &[0; 4]).unwrap().mode, Mode::Arm32);
        assert!(recognise("other.img", &[0; 4]).is_none());
        // A tiny 64-bit ELF for AArch64: one loaded, runnable part of 8 bytes at 0x400000.
        let mut elf = vec![0u8; 0x78];
        elf[..4].copy_from_slice(b"\x7FELF");
        elf[4] = 2;
        elf[5] = 1;
        elf[0x12..0x14].copy_from_slice(&183u16.to_le_bytes());
        elf[0x18..0x20].copy_from_slice(&0x400078u64.to_le_bytes());
        elf[0x20..0x28].copy_from_slice(&0x40u64.to_le_bytes());
        elf[0x36..0x38].copy_from_slice(&56u16.to_le_bytes());
        elf[0x38..0x3A].copy_from_slice(&1u16.to_le_bytes());
        elf[0x40..0x44].copy_from_slice(&1u32.to_le_bytes());
        elf[0x44..0x48].copy_from_slice(&5u32.to_le_bytes());
        elf[0x48..0x50].copy_from_slice(&0x78u64.to_le_bytes());
        elf[0x50..0x58].copy_from_slice(&0x400078u64.to_le_bytes());
        elf[0x60..0x68].copy_from_slice(&8u64.to_le_bytes());
        elf.extend([0x94, 0x02, 0x1E, 0x32, 0xC0, 0x03, 0x5F, 0xD6]);
        let program = recognise("hello", &elf).unwrap();
        assert_eq!((program.mode, program.parts.len(), program.parts[0].0), (Mode::A64, 1, 0x400078));
        let text = disassemble(&program);
        assert!(text.contains("orr w20, w20, #0x4") && text.contains("AArch64"), "{text}");
    }
}
