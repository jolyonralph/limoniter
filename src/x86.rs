//! x86 machine code, from the 8086 to today's Intel and AMD processors (SSE, AVX, AVX-512, AMX and the rest, as
//! the iced-x86 decoder knows them), disassembled into NASM-style source. What a file is says how: a DOS .COM
//! program (16-bit, at $100), a DOS .EXE (MZ: 16-bit, its load module, entered at CS:IP), a Windows program or
//! library (PE: 32- or 64-bit, its executable sections at their addresses, entered at its entry point and its
//! exports), or a Linux one (ELF: its executable segments, entered at its entry point and the functions its symbol
//! table names); anything else is taken as plain code in the mode chosen.
//!
//! The code is followed from where it starts, through every jump and call, so what's never reached stays data
//! (db). Places called become sub_ labels and places jumped to loc_ labels, or the names the file gives them.
//! 16-bit code is followed within its code segment: a near jump's target is an offset in the same segment, and a
//! far jump or call to an address in the load module (as it is before DOS relocates it) moves to that segment.

use iced_x86::{Decoder, DecoderOptions, FlowControl, Formatter, Instruction, NasmFormatter, OpKind, SymbolResolver, SymbolResult};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// A run of bytes at an address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub address: u64,
    pub bytes: Vec<u8>,
    pub name: String,
}

/// Code to disassemble: its mode (16, 32 or 64), its segments, where it starts, the names the file gives places,
/// and what it was found to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub bits: u32,
    pub segments: Vec<Segment>,
    pub starts: Vec<u64>,
    pub names: BTreeMap<u64, String>,
    /// 16-bit code: where the segment of each start begins (else 0).
    pub start_segments: Vec<u64>,
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

/// Plain code in a mode, at an address, from places it starts.
pub fn raw(bytes: &[u8], bits: u32, origin: u64, starts: &[u64]) -> Program {
    let starts = if starts.is_empty() { vec![origin] } else { starts.to_vec() };
    Program {
        bits,
        segments: vec![Segment { address: origin, bytes: bytes.to_vec(), name: String::new() }],
        start_segments: vec![0; starts.len()],
        starts,
        names: BTreeMap::new(),
        what: format!("{bits}-bit x86 code, {} bytes at {origin:#X}", bytes.len()),
    }
}

/// What an x86 file holds, if it's a kind Limoniter knows: PE, ELF, MZ, or (by its name) a DOS .COM program.
pub fn recognise(name: &str, bytes: &[u8]) -> Option<Program> {
    if bytes.starts_with(b"MZ") || bytes.starts_with(b"ZM") {
        return pe(bytes).or_else(|| mz(bytes));
    }
    if bytes.starts_with(b"\x7FELF") {
        return elf(bytes);
    }
    // A .COM program; an Atari's binary load file (which starts $FF $FF) shares the ending.
    if name.to_ascii_lowercase().ends_with(".com") && !bytes.is_empty() && bytes.len() <= 0xFF00 && !bytes.starts_with(&[0xFF, 0xFF]) {
        let mut program = raw(bytes, 16, 0x100, &[0x100]);
        program.what = format!("A DOS .COM program, {} bytes at $100", bytes.len());
        return Some(program);
    }
    None
}

/// A DOS .EXE: its load module, at 0 as it is before DOS relocates it, entered at CS:IP.
fn mz(bytes: &[u8]) -> Option<Program> {
    let last = u16_at(bytes, 2)? as usize;
    let pages = u16_at(bytes, 4)? as usize;
    let header = u16_at(bytes, 8)? as usize * 16;
    let (ip, cs) = (u16_at(bytes, 0x14)?, u16_at(bytes, 0x16)?);
    let size = if last == 0 { pages * 512 } else { pages.saturating_sub(1) * 512 + last };
    let module = bytes.get(header..size.min(bytes.len()).max(header))?.to_vec();
    let segment = u64::from(cs) * 16;
    let entry = segment + u64::from(ip);
    Some(Program {
        bits: 16,
        what: format!("A DOS program (MZ), its {} bytes of code and data from 0, entered at {cs:04X}:{ip:04X}", module.len()),
        segments: vec![Segment { address: 0, bytes: module, name: "load module".into() }],
        starts: vec![entry],
        start_segments: vec![segment],
        names: BTreeMap::from([(entry, "start".to_string())]),
    })
}

/// A Windows program or library (PE): its executable sections at their addresses, from its entry point and its
/// exports.
fn pe(bytes: &[u8]) -> Option<Program> {
    let at = u32_at(bytes, 0x3C)? as usize;
    if bytes.get(at..at + 4)? != b"PE\0\0" {
        return None;
    }
    let machine = u16_at(bytes, at + 4)?;
    let bits = match machine {
        0x14C => 32,
        0x8664 => 64,
        _ => return None,
    };
    let sections = u16_at(bytes, at + 6)? as usize;
    let optional = at + 24;
    let optional_size = u16_at(bytes, at + 20)? as usize;
    let plus = u16_at(bytes, optional)? == 0x20B;
    let entry_rva = u32_at(bytes, optional + 16)?;
    let base = if plus { u64_at(bytes, optional + 24)? } else { u64::from(u32_at(bytes, optional + 28)?) };
    let mut segments = Vec::new();
    // Every section, to find RVAs in; the executable ones are disassembled.
    let mut all = Vec::new();
    for n in 0..sections {
        let s = optional + optional_size + n * 40;
        let name: String = bytes.get(s..s + 8)?.iter().take_while(|b| **b != 0).map(|b| char::from(*b)).collect();
        let (virtual_size, rva, raw_size, raw_at, flags) = (u32_at(bytes, s + 8)?, u32_at(bytes, s + 12)?, u32_at(bytes, s + 16)?, u32_at(bytes, s + 20)?, u32_at(bytes, s + 36)?);
        let size = if virtual_size == 0 { raw_size } else { raw_size.min(virtual_size) } as usize;
        let data = bytes.get(raw_at as usize..).map(|b| b[..size.min(b.len())].to_vec()).unwrap_or_default();
        all.push((rva, virtual_size.max(raw_size), raw_at, raw_size));
        if flags & 0x2000_0000 != 0 && !data.is_empty() {
            segments.push(Segment { address: base + u64::from(rva), bytes: data, name });
        }
    }
    let file_at = |rva: u32| all.iter().find(|(start, size, ..)| rva >= *start && rva < start + size).map(|(start, _, raw_at, _)| (rva - start + raw_at) as usize);
    let mut names = BTreeMap::new();
    let mut starts = Vec::new();
    if entry_rva != 0 {
        starts.push(base + u64::from(entry_rva));
        names.insert(base + u64::from(entry_rva), "start".to_string());
    }
    // The export directory: each function exported by name.
    let directories = optional + if plus { 112 } else { 96 };
    if let (Some(export_rva), Some(export_size)) = (u32_at(bytes, directories), u32_at(bytes, directories + 4))
        && export_rva != 0
        && let Some(e) = file_at(export_rva)
    {
        let (count, functions, name_list, ordinals) = (u32_at(bytes, e + 24).unwrap_or(0) as usize, u32_at(bytes, e + 28).unwrap_or(0), u32_at(bytes, e + 32).unwrap_or(0), u32_at(bytes, e + 36).unwrap_or(0));
        for n in 0..count.min(65536) {
            let (Some(name_at), Some(ordinal_at)) = (file_at(name_list + n as u32 * 4), file_at(ordinals + n as u32 * 2)) else { break };
            let (Some(name_rva), Some(ordinal)) = (u32_at(bytes, name_at), u16_at(bytes, ordinal_at)) else { break };
            let Some(function_rva) = file_at(functions + u32::from(ordinal) * 4).and_then(|f| u32_at(bytes, f)) else { continue };
            // A forwarder points back into the export directory, at a name, not at code.
            if function_rva >= export_rva && function_rva < export_rva + export_size {
                continue;
            }
            let Some(text) = file_at(name_rva).and_then(|t| bytes.get(t..)) else { continue };
            let name: String = text.iter().take_while(|b| **b != 0).take(200).map(|b| char::from(*b)).collect();
            let address = base + u64::from(function_rva);
            starts.push(address);
            names.insert(address, name);
        }
    }
    // A 64-bit program's exception directory: where every function that has unwind information starts.
    if plus
        && let (Some(table_rva), Some(table_size)) = (u32_at(bytes, directories + 24), u32_at(bytes, directories + 28))
        && let Some(table) = file_at(table_rva)
    {
        for entry in (0..table_size as usize / 12).map(|n| table + n * 12) {
            match u32_at(bytes, entry) {
                Some(begin) if begin != 0 => starts.push(base + u64::from(begin)),
                _ => break,
            }
        }
    }
    let kind = if u16_at(bytes, at + 22).is_some_and(|c| c & 0x2000 != 0) { "library (DLL)" } else { "program" };
    let what = format!("A {bits}-bit Windows {kind} (PE), {} executable section{} from {base:#X}, entered at {:#X}", segments.len(), if segments.len() == 1 { "" } else { "s" }, base + u64::from(entry_rva));
    Some(Program { bits, segments, start_segments: vec![0; starts.len()], starts, names, what })
}

/// A Linux program or library (ELF, x86 or x86-64): its executable segments, from its entry point and the
/// functions its symbol table names.
fn elf(bytes: &[u8]) -> Option<Program> {
    let wide = *bytes.get(4)? == 2;
    if *bytes.get(5)? != 1 {
        return None;
    }
    let machine = u16_at(bytes, 18)?;
    let bits = match machine {
        3 => 32,
        62 => 64,
        _ => return None,
    };
    let word = |at: usize| if wide { u64_at(bytes, at) } else { u32_at(bytes, at).map(u64::from) };
    let entry = word(24)?;
    let (ph_at, sh_at) = (word(if wide { 32 } else { 28 })? as usize, word(if wide { 40 } else { 32 })? as usize);
    let (ph_size, ph_count) = (u16_at(bytes, if wide { 54 } else { 42 })? as usize, u16_at(bytes, if wide { 56 } else { 44 })? as usize);
    let (sh_size, sh_count) = (u16_at(bytes, if wide { 58 } else { 46 })? as usize, u16_at(bytes, if wide { 60 } else { 48 })? as usize);
    let mut segments = Vec::new();
    for n in 0..ph_count {
        let p = ph_at + n * ph_size;
        let kind = u32_at(bytes, p)?;
        let (flags, offset, address, size) = if wide { (u32_at(bytes, p + 4)?, u64_at(bytes, p + 8)?, u64_at(bytes, p + 16)?, u64_at(bytes, p + 32)?) } else { (u32_at(bytes, p + 24)?, u64::from(u32_at(bytes, p + 4)?), u64::from(u32_at(bytes, p + 8)?), u64::from(u32_at(bytes, p + 16)?)) };
        if kind == 1 && flags & 1 != 0 {
            let data = bytes.get(offset as usize..).map(|b| b[..(size as usize).min(b.len())].to_vec()).unwrap_or_default();
            segments.push(Segment { address, bytes: data, name: format!("segment {n}") });
        }
    }
    let mut names = BTreeMap::new();
    let mut starts = vec![entry];
    names.insert(entry, "_start".to_string());
    // The symbol tables (.symtab, and .dynsym), each with the string table its section links to.
    for n in 0..sh_count {
        let s = sh_at + n * sh_size;
        let Some(kind) = u32_at(bytes, s + 4) else { break };
        if kind != 2 && kind != 11 {
            continue;
        }
        let (offset, size, link, entry_size) = if wide {
            (u64_at(bytes, s + 24)? as usize, u64_at(bytes, s + 32)? as usize, u32_at(bytes, s + 40)? as usize, u64_at(bytes, s + 56)? as usize)
        } else {
            (u32_at(bytes, s + 16)? as usize, u32_at(bytes, s + 20)? as usize, u32_at(bytes, s + 24)? as usize, u32_at(bytes, s + 36)? as usize)
        };
        let strings_at = word(sh_at + link * sh_size + if wide { 24 } else { 16 })? as usize;
        for sym in (0..size / entry_size.max(1)).map(|i| offset + i * entry_size) {
            let (name_at, info, value) = if wide { (u32_at(bytes, sym)?, *bytes.get(sym + 4)?, u64_at(bytes, sym + 8)?) } else { (u32_at(bytes, sym)?, *bytes.get(sym + 12)?, u64::from(u32_at(bytes, sym + 4)?)) };
            // Functions defined here.
            if info & 0xF != 2 || value == 0 {
                continue;
            }
            let Some(text) = bytes.get(strings_at + name_at as usize..) else { continue };
            let name: String = text.iter().take_while(|b| **b != 0).take(200).map(|b| char::from(*b)).collect();
            if !name.is_empty() && segments.iter().any(|s| value >= s.address && value < s.address + s.bytes.len() as u64) {
                starts.push(value);
                names.entry(value).or_insert(name);
            }
        }
    }
    let what = format!("A {bits}-bit Linux program or library (ELF), {} executable segment{}, entered at {entry:#X}", segments.len(), if segments.len() == 1 { "" } else { "s" });
    Some(Program { bits, segments, start_segments: vec![0; starts.len()], starts, names, what })
}

/// How a byte was taken.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Seen {
    Data,
    Start,
    Inside,
}

/// The labels the formatter writes in place of addresses, for the segment of the instruction being written.
struct Labels {
    names: BTreeMap<u64, String>,
    segment: u64,
}

struct Resolver(Rc<RefCell<Labels>>);

impl SymbolResolver for Resolver {
    fn symbol(&mut self, instruction: &Instruction, operand: u32, _: Option<u32>, address: u64, _: u32) -> Option<SymbolResult<'_>> {
        let labels = self.0.borrow();
        // Branches, and memory addressed from RIP or EIP; an immediate that happens to match isn't an address.
        let linear = match instruction.op_kind(operand) {
            OpKind::NearBranch16 => labels.segment + address,
            OpKind::NearBranch32 | OpKind::NearBranch64 => address,
            OpKind::FarBranch16 => u64::from(instruction.far_branch_selector()) * 16 + address,
            OpKind::Memory if instruction.is_ip_rel_memory_operand() => address,
            _ => return None,
        };
        labels.names.get(&linear).map(|name| SymbolResult::with_string(address, name.clone()))
    }
}

/// The source for a program.
pub fn disassemble(program: &Program) -> String {
    let bits = program.bits;
    let segment_of = |a: u64| program.segments.iter().position(|s| a >= s.address && a < s.address + s.bytes.len() as u64);
    let mut seen: Vec<Vec<Seen>> = program.segments.iter().map(|s| vec![Seen::Data; s.bytes.len()]).collect();
    // Each instruction found: where, the segment base it runs in (16-bit), and it.
    let mut found: BTreeMap<u64, (u64, Instruction)> = BTreeMap::new();
    let mut called: BTreeSet<u64> = BTreeSet::new();
    let mut jumped: BTreeSet<u64> = BTreeSet::new();
    let mut queue: Vec<(u64, u64)> = program.starts.iter().copied().zip(program.start_segments.iter().copied()).collect();
    while let Some((mut at, cs)) = queue.pop() {
        while let Some(s) = segment_of(at) {
            let segment = &program.segments[s];
            let offset = (at - segment.address) as usize;
            if seen[s][offset] != Seen::Data {
                break;
            }
            let mut decoder = Decoder::with_ip(bits, &segment.bytes[offset..], at - cs, DecoderOptions::NONE);
            let instruction = decoder.decode();
            let length = instruction.len();
            if instruction.is_invalid() || (1..length).any(|i| seen[s].get(offset + i) != Some(&Seen::Data)) {
                break;
            }
            seen[s][offset] = Seen::Start;
            for i in 1..length {
                seen[s][offset + i] = Seen::Inside;
            }
            let flow = instruction.flow_control();
            if matches!(flow, FlowControl::UnconditionalBranch | FlowControl::ConditionalBranch | FlowControl::Call | FlowControl::XbeginXabortXend) {
                let target = match instruction.op0_kind() {
                    OpKind::NearBranch16 | OpKind::NearBranch32 | OpKind::NearBranch64 => Some((cs + instruction.near_branch_target(), cs)),
                    OpKind::FarBranch16 if bits == 16 => {
                        let to = u64::from(instruction.far_branch_selector()) * 16;
                        Some((to + u64::from(instruction.far_branch16()), to))
                    }
                    _ => None,
                };
                if let Some((target, target_cs)) = target.filter(|(t, _)| segment_of(*t).is_some()) {
                    if flow == FlowControl::Call { called.insert(target) } else { jumped.insert(target) };
                    queue.push((target, target_cs));
                }
            }
            found.insert(at, (cs, instruction));
            if matches!(flow, FlowControl::UnconditionalBranch | FlowControl::IndirectBranch | FlowControl::Return | FlowControl::Exception) {
                break;
            }
            at += length as u64;
        }
    }
    // Labels: the file's own names, then sub_ for what's called and loc_ for what's jumped to.
    let mut names = program.names.clone();
    names.retain(|a, _| found.contains_key(a) || segment_of(*a).is_some());
    for a in &called {
        names.entry(*a).or_insert_with(|| format!("sub_{a:X}"));
    }
    for a in &jumped {
        names.entry(*a).or_insert_with(|| format!("loc_{a:X}"));
    }
    let labels = Rc::new(RefCell::new(Labels { names: names.clone(), segment: 0 }));
    let mut formatter = NasmFormatter::with_options(Some(Box::new(Resolver(labels.clone()))), None);
    formatter.options_mut().set_uppercase_hex(true);
    formatter.options_mut().set_space_after_operand_separator(true);
    formatter.options_mut().set_first_operand_char_index(8);

    let mut lines = vec![format!("; {}", program.what), "; Disassembled by Limoniter: addresses and bytes are in the comments.".to_string(), String::new(), format!("        bits {bits}")];
    for (s, segment) in program.segments.iter().enumerate() {
        lines.push(String::new());
        let name = if segment.name.is_empty() { String::new() } else { format!(" ({})", segment.name) };
        lines.push(format!("; {:#X} to {:#X}{name}", segment.address, segment.address + segment.bytes.len() as u64));
        if program.segments.len() == 1 {
            lines.push(format!("        org {:#X}", segment.address));
        }
        let mut offset = 0usize;
        while offset < segment.bytes.len() {
            let at = segment.address + offset as u64;
            if let Some(name) = names.get(&at) {
                if called.contains(&at) || program.names.contains_key(&at) {
                    lines.push(String::new());
                }
                lines.push(format!("{name}:"));
            }
            if let Some((cs, instruction)) = found.get(&at) {
                labels.borrow_mut().segment = *cs;
                let mut text = String::new();
                formatter.format(instruction, &mut text);
                let length = instruction.len();
                let hex: Vec<String> = segment.bytes[offset..offset + length].iter().map(|b| format!("{b:02X}")).collect();
                let place = if bits == 16 && program.segments[0].name == "load module" { format!("{:04X}:{:04X}", cs / 16, at - cs) } else { format!("{at:08X}") };
                lines.push(format!("        {text:<48}; {place}  {}", hex.join(" ")));
                if matches!(instruction.flow_control(), FlowControl::UnconditionalBranch | FlowControl::IndirectBranch | FlowControl::Return) {
                    lines.push(String::new());
                }
                offset += length;
                continue;
            }
            // Data up to the next label or instruction, sixteen bytes a line.
            let mut run = Vec::new();
            while offset + run.len() < segment.bytes.len() && run.len() < 16 {
                let a = at + run.len() as u64;
                if !run.is_empty() && (names.contains_key(&a) || seen[s][offset + run.len()] != Seen::Data) {
                    break;
                }
                run.push(segment.bytes[offset + run.len()]);
            }
            let items: Vec<String> = run.iter().map(|b| format!("0x{b:02X}")).collect();
            lines.push(format!("        {:<48}; {at:08X}", format!("db {}", items.join(", "))));
            offset += run.len();
        }
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The source with each run of spaces made one, to look for instructions in.
    fn squashed(program: &Program) -> String {
        disassemble(program).lines().map(|l| l.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn a_com_program_is_followed_from_0100() {
        // mov ah,9 / mov dx,msg / int 21h / mov ax,4C00h / int 21h / jmp $ ... then a message that's never run.
        let mut com = vec![0xB4, 0x09, 0xBA, 0x0E, 0x01, 0xCD, 0x21, 0xB8, 0x00, 0x4C, 0xCD, 0x21, 0xEB, 0xFE];
        com.extend(b"Hi$");
        let program = recognise("HELLO.COM", &com).unwrap();
        assert_eq!((program.bits, program.starts.clone()), (16, vec![0x100]));
        let source = squashed(&program);
        assert!(source.contains("org 0x100"), "{source}");
        assert!(source.contains("mov ah, 9") && source.contains("int 21h"), "{source}");
        assert!(source.contains("loc_10C:") && source.contains("jmp short loc_10C"), "{source}");
        assert!(source.contains("db 0x48, 0x69, 0x24"), "the message is data: {source}");
        // An Atari binary load file with the same ending isn't one.
        assert!(recognise("GAME.COM", &[0xFF, 0xFF, 0, 6, 0, 6, 0x60]).is_none());
    }

    #[test]
    fn modern_instructions_are_known_in_64_bit_code() {
        // vaddps zmm1{k1}, zmm2, zmm3 (AVX-512) / vpdpbusd ymm0, ymm1, ymm2 (AVX-VNNI) / ret
        let code = [0x62, 0xF1, 0x6C, 0x49, 0x58, 0xCB, 0xC4, 0xE2, 0x75, 0x50, 0xC2, 0xC3];
        let source = squashed(&raw(&code, 64, 0x1000, &[]));
        assert!(source.contains("vaddps zmm1{k1}, zmm2, zmm3"), "{source}");
        assert!(source.contains("vpdpbusd") && source.contains("ret"), "{source}");
    }

    #[test]
    fn calls_get_sub_labels_and_what_follows_a_return_is_data() {
        // call sub / ret / sub: xor eax,eax / ret / 0xCC padding
        let code = [0xE8, 0x01, 0x00, 0x00, 0x00, 0xC3, 0x31, 0xC0, 0xC3, 0xCC];
        let source = squashed(&raw(&code, 32, 0x401000, &[]));
        assert!(source.contains("sub_401006:") && source.contains("call sub_401006"), "{source}");
        assert!(source.contains("db 0xCC"), "{source}");
    }

    /// A small PE: headers, then one executable section at RVA $1000 holding `ret`, exported as Thing.
    fn tiny_pe(bits: u32) -> Vec<u8> {
        let mut f = vec![0u8; 0x400];
        f[0..2].copy_from_slice(b"MZ");
        f[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        f[0x80..0x84].copy_from_slice(b"PE\0\0");
        let plus = bits == 64;
        f[0x84..0x86].copy_from_slice(&(if plus { 0x8664u16 } else { 0x14C }).to_le_bytes());
        f[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
        let optional_size: u16 = if plus { 240 } else { 224 };
        f[0x94..0x96].copy_from_slice(&optional_size.to_le_bytes());
        let o = 0x98;
        f[o..o + 2].copy_from_slice(&(if plus { 0x20Bu16 } else { 0x10B }).to_le_bytes());
        f[o + 16..o + 20].copy_from_slice(&0x1000u32.to_le_bytes());
        if plus {
            f[o + 24..o + 32].copy_from_slice(&0x1_4000_0000u64.to_le_bytes());
        } else {
            f[o + 28..o + 32].copy_from_slice(&0x40_0000u32.to_le_bytes());
        }
        // The export directory, in the second section.
        let directories = o + if plus { 112 } else { 96 };
        f[directories..directories + 4].copy_from_slice(&0x2000u32.to_le_bytes());
        f[directories + 4..directories + 8].copy_from_slice(&0x100u32.to_le_bytes());
        let sections = o + usize::from(optional_size);
        for (n, (name, rva, raw_at, flags)) in [(b".text\0\0\0", 0x1000u32, 0x200u32, 0x6000_0020u32), (b".edata\0\0", 0x2000, 0x300, 0x4000_0040)].iter().enumerate() {
            let s = sections + n * 40;
            f[s..s + 8].copy_from_slice(*name);
            f[s + 8..s + 12].copy_from_slice(&0x100u32.to_le_bytes());
            f[s + 12..s + 16].copy_from_slice(&rva.to_le_bytes());
            f[s + 16..s + 20].copy_from_slice(&0x100u32.to_le_bytes());
            f[s + 20..s + 24].copy_from_slice(&raw_at.to_le_bytes());
            f[s + 36..s + 40].copy_from_slice(&flags.to_le_bytes());
        }
        // .text: ret at the entry point, then Thing: mov eax,1 / ret.
        f[0x200] = 0xC3;
        f[0x201..0x207].copy_from_slice(&[0xB8, 1, 0, 0, 0, 0xC3]);
        // A function nothing calls, found only in a 64-bit program's .pdata: xor eax,eax / ret.
        f[0x207..0x20A].copy_from_slice(&[0x31, 0xC0, 0xC3]);
        if plus {
            f[directories + 24..directories + 28].copy_from_slice(&0x2080u32.to_le_bytes());
            f[directories + 28..directories + 32].copy_from_slice(&12u32.to_le_bytes());
            f[0x380..0x384].copy_from_slice(&0x1007u32.to_le_bytes());
            f[0x384..0x388].copy_from_slice(&0x100Au32.to_le_bytes());
        }
        // .edata: one function, named, at RVA $1001.
        let e = 0x300;
        f[e + 24..e + 28].copy_from_slice(&1u32.to_le_bytes());
        f[e + 28..e + 32].copy_from_slice(&0x2040u32.to_le_bytes());
        f[e + 32..e + 36].copy_from_slice(&0x2050u32.to_le_bytes());
        f[e + 36..e + 40].copy_from_slice(&0x2060u32.to_le_bytes());
        f[0x340..0x344].copy_from_slice(&0x1001u32.to_le_bytes());
        f[0x350..0x354].copy_from_slice(&0x2070u32.to_le_bytes());
        f[0x370..0x376].copy_from_slice(b"Thing\0");
        f
    }

    #[test]
    fn a_pe_file_is_disassembled_from_its_entry_point_and_exports() {
        for (bits, base) in [(32, 0x40_0000u64), (64, 0x1_4000_0000)] {
            let program = recognise("thing.dll", &tiny_pe(bits)).unwrap();
            assert_eq!(program.bits, bits);
            assert_eq!(program.segments.len(), 1, "only .text is code");
            assert_eq!(program.segments[0].address, base + 0x1000);
            let source = squashed(&program);
            assert!(source.contains("start:") && source.contains("Thing:"), "{source}");
            assert!(source.contains("mov eax, 1"), "{source}");
            if bits == 64 {
                assert_eq!(program.starts, vec![base + 0x1000, base + 0x1001, base + 0x1007]);
                assert!(source.contains("xor eax, eax"), "found from .pdata: {source}");
            } else {
                assert_eq!(program.starts, vec![base + 0x1000, base + 0x1001]);
                assert!(source.contains("db 0x31, 0xC0, 0xC3"), "nothing reaches it: {source}");
            }
        }
    }

    #[test]
    fn a_dos_exe_is_entered_at_cs_ip_and_far_calls_move_segment() {
        // Header of 2 paragraphs; the module is 0x30 bytes; CS:IP = 0001:0000.
        let mut exe = vec![0u8; 32];
        exe[0..2].copy_from_slice(b"MZ");
        let size = 32 + 0x30;
        exe[2..4].copy_from_slice(&(size as u16 % 512).to_le_bytes());
        exe[4..6].copy_from_slice(&1u16.to_le_bytes());
        exe[8..10].copy_from_slice(&2u16.to_le_bytes());
        exe[0x14..0x16].copy_from_slice(&0u16.to_le_bytes());
        exe[0x16..0x18].copy_from_slice(&1u16.to_le_bytes());
        let mut module = vec![0x90u8; 0x30];
        // At 0001:0000 (linear $10): call far 0002:0000 / mov ax,4C00h / int 21h / jmp $.
        module[0x10..0x1C].copy_from_slice(&[0x9A, 0x00, 0x00, 0x02, 0x00, 0xB8, 0x00, 0x4C, 0xCD, 0x21, 0xEB, 0xFE]);
        // At 0002:0000 (linear $20): jmp short +0 (to 0002:0002) / retf.
        module[0x20..0x23].copy_from_slice(&[0xEB, 0x00, 0xCB]);
        exe.extend(module);
        let program = recognise("GAME.EXE", &exe).unwrap();
        assert_eq!((program.bits, program.starts.clone()), (16, vec![0x10]));
        let source = squashed(&program);
        assert!(source.contains("start:") && source.contains("0001:0000"), "{source}");
        assert!(source.contains("sub_20:") && source.contains("0002:0000"), "{source}");
        assert!(source.contains("loc_22:") && source.contains("0002:0002"), "the near jump stays in segment 2: {source}");
    }
}

/// A real program, where LIMONITER_X86_FILE names one: it's recognised, followed, and every byte of its code
/// is in the source once, as an instruction or as data.
#[test]
fn a_real_program_is_disassembled() {
    let Some(path) = std::env::var_os("LIMONITER_X86_FILE") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    let bytes = std::fs::read(&path).unwrap();
    let program = recognise(&path.to_string_lossy(), &bytes).expect("an x86 program");
    let began = std::time::Instant::now();
    let source = disassemble(&program);
    let instructions = source.lines().filter(|l| l.starts_with("        ") && !l.trim_start().starts_with("db ") && l.contains(';')).count();
    eprintln!("{}: {} lines, {instructions} instructions, {} starts, in {:?}", program.what, source.lines().count(), program.starts.len(), began.elapsed());
    for line in source.lines().filter(|l| !l.trim().is_empty()).skip(4).take(40) {
        eprintln!("{line}");
    }
    assert!(instructions > 100);
}
